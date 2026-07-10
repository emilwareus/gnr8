import { ApiError } from "./errors";
import * as models from "./models";

export interface RequestOptions {
  timeoutMs?: number;
  maxRetries?: number;
  idempotencyKey?: string;
  metadata?: Record<string, string>;
}

export interface HookContext {
  operationId: string;
  method: string;
  pathTemplate: string;
  url: string;
  headers: Record<string, string>;
  requestMetadata: Record<string, string>;
  status?: number;
  responseHeaders?: Headers;
}

export type RequestHook = (
  context: HookContext,
  init: RequestInit,
) => void | Promise<void>;
export type ResponseHook = (
  context: HookContext,
  response: Response,
) => void | Promise<void>;
export type ErrorHook = (
  context: HookContext,
  error: unknown,
) => void | Promise<void>;

export interface ClientHooks {
  request?: RequestHook[];
  response?: ResponseHook[];
  error?: ErrorHook[];
}

export interface ClientOptions {
  baseUrl: string;
  fetch?: typeof fetch;
  apiKey?: string;
  apiKeys?: Record<string, string>;
  timeoutMs?: number;
  maxRetries?: number;
  hooks?: ClientHooks;
}

interface RuntimeRequestContext {
  operationId: string;
  pathTemplate: string;
  idempotent?: boolean;
  idempotencyKeyHeader?: string;
}

export class Client {
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;
  private readonly apiKey?: string;
  private readonly apiKeys: Record<string, string>;
  private readonly timeoutMs?: number;
  private readonly maxRetries: number;
  private readonly retryStatuses: Set<number>;
  private readonly retryUnsafeMethods: boolean;
  private readonly hooks: Required<ClientHooks>;

  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.fetchFn = opts.fetch ?? fetch;
    this.apiKey = opts.apiKey;
    this.apiKeys = opts.apiKeys ?? {};
    this.timeoutMs = opts.timeoutMs ?? 30000;
    this.maxRetries = opts.maxRetries ?? 0;
    this.retryStatuses = new Set<number>([408, 429]);
    this.retryUnsafeMethods = false;
    this.hooks = {
      request: opts.hooks?.request ?? [],
      response: opts.hooks?.response ?? [],
      error: opts.hooks?.error ?? [],
    };
  }

  _apiKey(...names: string[]): string | undefined {
    for (const name of names) {
      const value = this.apiKeys[name];
      if (value !== undefined) {
        return value;
      }
    }
    return this.apiKey;
  }

  private _encodeBody(body: unknown): BodyInit | undefined {
    if (body === undefined) {
      return undefined;
    }
    if (
      body instanceof URLSearchParams ||
      body instanceof FormData ||
      body instanceof Blob ||
      body instanceof ArrayBuffer ||
      typeof body === "string"
    ) {
      return body;
    }
    if (ArrayBuffer.isView(body)) {
      return new Blob([body as unknown as BlobPart]);
    }
    return JSON.stringify(body);
  }

  private _formBody(body: unknown): URLSearchParams {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(
      body as Record<string, unknown>,
    )) {
      if (value === undefined || value === null) {
        continue;
      }
      if (Array.isArray(value)) {
        for (const item of value) {
          params.append(key, String(item));
        }
      } else {
        params.set(key, String(value));
      }
    }
    return params;
  }

  private _multipartBody(body: unknown): FormData {
    const form = new FormData();
    for (const [key, value] of Object.entries(
      body as Record<string, unknown>,
    )) {
      if (value === undefined || value === null) {
        continue;
      }
      if (Array.isArray(value)) {
        for (const item of value) {
          this._appendMultipartValue(form, key, item);
        }
      } else {
        this._appendMultipartValue(form, key, value);
      }
    }
    return form;
  }

  private _appendMultipartValue(
    form: FormData,
    key: string,
    value: unknown,
  ): void {
    if (value === undefined || value === null) {
      return;
    }
    if (value instanceof Blob) {
      form.append(key, value);
    } else if (value instanceof ArrayBuffer || ArrayBuffer.isView(value)) {
      form.append(key, new Blob([value as BlobPart]), key);
    } else {
      form.append(key, String(value));
    }
  }

  async _request(
    method: string,
    path: string,
    headers: Record<string, string>,
    body?: unknown,
    requestContext?: RuntimeRequestContext,
    options: RequestOptions = {},
  ): Promise<Response> {
    const context = requestContext ?? { operationId: "", pathTemplate: path };
    const url = `${this.baseUrl}${path}`;
    const requestMetadata = options.metadata ?? {};
    if (context.idempotent === true && options.idempotencyKey !== undefined) {
      headers[context.idempotencyKeyHeader ?? "Idempotency-Key"] =
        options.idempotencyKey;
    }
    const maxRetries = Math.max(0, options.maxRetries ?? this.maxRetries);
    const retryAttempts =
      this.retryUnsafeMethods ||
      context.idempotent === true ||
      this._retryableMethod(method)
        ? maxRetries
        : 0;
    const timeoutMs = options.timeoutMs ?? this.timeoutMs;
    const bodyPayload = this._encodeBody(body);
    let lastError: unknown = undefined;
    for (let attempt = 0; attempt <= retryAttempts; attempt += 1) {
      const controller =
        timeoutMs !== undefined && timeoutMs > 0
          ? new AbortController()
          : undefined;
      const timeoutId =
        controller === undefined
          ? undefined
          : setTimeout(() => controller.abort(), timeoutMs);
      const init: RequestInit = {
        method,
        headers,
        body: bodyPayload,
        signal: controller?.signal,
      };
      const hookContext: HookContext = {
        operationId: context.operationId,
        method,
        pathTemplate: context.pathTemplate,
        url,
        headers: { ...headers },
        requestMetadata,
      };
      try {
        for (const hook of this.hooks.request) {
          await hook(hookContext, init);
        }
      } catch (error) {
        if (timeoutId !== undefined) {
          clearTimeout(timeoutId);
        }
        for (const hook of this.hooks.error) {
          await hook(hookContext, error);
        }
        throw error;
      }
      let response: Response | undefined = undefined;
      try {
        response = await this.fetchFn(url, init);
        if (timeoutId !== undefined) {
          clearTimeout(timeoutId);
        }
      } catch (error) {
        if (timeoutId !== undefined) {
          clearTimeout(timeoutId);
        }
        lastError = error;
        if (attempt < retryAttempts) {
          continue;
        }
        for (const hook of this.hooks.error) {
          await hook(hookContext, error);
        }
        throw error;
      }
      if (response === undefined) {
        throw new Error("request failed without response");
      }
      hookContext.status = response.status;
      hookContext.responseHeaders = response.headers;
      try {
        for (const hook of this.hooks.response) {
          await hook(hookContext, response);
        }
      } catch (error) {
        for (const hook of this.hooks.error) {
          await hook(hookContext, error);
        }
        throw error;
      }
      if (this._shouldRetryStatus(response.status) && attempt < retryAttempts) {
        await this._sleep(this._retryDelayMs(response));
        continue;
      }
      if (response.status < 200 || response.status >= 300) {
        const error = new ApiError(response.status, {
          headers: response.headers,
        });
        for (const hook of this.hooks.error) {
          await hook(hookContext, error);
        }
      }
      return response;
    }
    throw lastError ?? new Error("request failed without response");
  }

  private _retryableMethod(method: string): boolean {
    return (
      method === "GET" ||
      method === "HEAD" ||
      method === "OPTIONS" ||
      method === "PUT" ||
      method === "DELETE"
    );
  }

  private _shouldRetryStatus(status: number): boolean {
    return this.retryStatuses.has(status) || status >= 500;
  }

  private _retryDelayMs(response: Response): number {
    const retryAfter = response.headers.get("Retry-After");
    if (retryAfter === null) {
      return 0;
    }
    const seconds = Number.parseInt(retryAfter, 10);
    return Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : 0;
  }

  private async _sleep(ms: number): Promise<void> {
    if (ms <= 0) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, ms));
  }

  async listBooks(
    genre: string,
    cursor?: string,
    sort?: string,
    options?: RequestOptions,
  ): Promise<models.ListBooksResponse> {
    let path = `/books/`;
    const searchParams = new URLSearchParams();
    searchParams.set("genre", String(genre));
    if (cursor !== undefined) {
      searchParams.set("cursor", String(cursor));
    }
    if (sort !== undefined) {
      searchParams.set("sort", String(sort));
    }
    const qs = searchParams.toString();
    if (qs) {
      path = path + "?" + qs;
    }
    const headers: Record<string, string> = {};
    const res = await this._request(
      "GET",
      path,
      headers,
      undefined,
      {
        operationId: "listBooks",
        pathTemplate: "/",
        idempotent: false,
        idempotencyKeyHeader: "Idempotency-Key",
      },
      options,
    );
    if (res.status < 200 || res.status >= 300) {
      const rawBody = await res.text();
      let jsonBody: unknown = null;
      try {
        jsonBody = rawBody ? JSON.parse(rawBody) : null;
      } catch {
        jsonBody = null;
      }
      let errorBody: unknown = jsonBody;
      throw new ApiError(res.status, {
        headers: res.headers,
        requestId: res.headers.get("x-request-id") ?? undefined,
        rawBody,
        jsonBody,
        body: errorBody,
      });
    }
    if (res.status === 200) {
      return (await res.json()) as models.ListBooksResponse;
    }
    throw new ApiError(res.status);
  }

  async createBook(
    body: models.BookDto,
    options?: RequestOptions,
  ): Promise<models.CreatedMessage> {
    let path = `/books/`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this._request(
      "POST",
      path,
      headers,
      body,
      {
        operationId: "createBook",
        pathTemplate: "/",
        idempotent: false,
        idempotencyKeyHeader: "Idempotency-Key",
      },
      options,
    );
    if (res.status < 200 || res.status >= 300) {
      const rawBody = await res.text();
      let jsonBody: unknown = null;
      try {
        jsonBody = rawBody ? JSON.parse(rawBody) : null;
      } catch {
        jsonBody = null;
      }
      let errorBody: unknown = jsonBody;
      throw new ApiError(res.status, {
        headers: res.headers,
        requestId: res.headers.get("x-request-id") ?? undefined,
        rawBody,
        jsonBody,
        body: errorBody,
      });
    }
    if (res.status === 201) {
      return (await res.json()) as models.CreatedMessage;
    }
    throw new ApiError(res.status);
  }

  async getBook(
    bookId: number,
    fmt?: models.BookFormat,
    options?: RequestOptions,
  ): Promise<models.BookOrError> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const searchParams = new URLSearchParams();
    if (fmt !== undefined) {
      searchParams.set("fmt", String(fmt));
    }
    const qs = searchParams.toString();
    if (qs) {
      path = path + "?" + qs;
    }
    const headers: Record<string, string> = {};
    const res = await this._request(
      "GET",
      path,
      headers,
      undefined,
      {
        operationId: "getBook",
        pathTemplate: "/{bookId}",
        idempotent: false,
        idempotencyKeyHeader: "Idempotency-Key",
      },
      options,
    );
    if (res.status < 200 || res.status >= 300) {
      const rawBody = await res.text();
      let jsonBody: unknown = null;
      try {
        jsonBody = rawBody ? JSON.parse(rawBody) : null;
      } catch {
        jsonBody = null;
      }
      let errorBody: unknown = jsonBody;
      throw new ApiError(res.status, {
        headers: res.headers,
        requestId: res.headers.get("x-request-id") ?? undefined,
        rawBody,
        jsonBody,
        body: errorBody,
      });
    }
    if (res.status === 200) {
      return (await res.json()) as models.BookOrError;
    }
    throw new ApiError(res.status);
  }

  async updateBook(
    bookId: number,
    body: models.BookFilters,
    options?: RequestOptions,
  ): Promise<models.CreatedMessage> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this._request(
      "PUT",
      path,
      headers,
      body,
      {
        operationId: "updateBook",
        pathTemplate: "/{bookId}",
        idempotent: false,
        idempotencyKeyHeader: "Idempotency-Key",
      },
      options,
    );
    if (res.status < 200 || res.status >= 300) {
      const rawBody = await res.text();
      let jsonBody: unknown = null;
      try {
        jsonBody = rawBody ? JSON.parse(rawBody) : null;
      } catch {
        jsonBody = null;
      }
      let errorBody: unknown = jsonBody;
      throw new ApiError(res.status, {
        headers: res.headers,
        requestId: res.headers.get("x-request-id") ?? undefined,
        rawBody,
        jsonBody,
        body: errorBody,
      });
    }
    if (res.status === 200) {
      return (await res.json()) as models.CreatedMessage;
    }
    throw new ApiError(res.status);
  }
}
