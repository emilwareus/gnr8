import { ApiError } from "./errors";
import * as models from "./models";

export interface ClientOptions {
  baseUrl: string;
  fetch?: typeof fetch;
  apiKey?: string;
  apiKeys?: Record<string, string>;
}

export class Client {
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;
  private readonly apiKey?: string;
  private readonly apiKeys: Record<string, string>;

  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.fetchFn = opts.fetch ?? fetch;
    this.apiKey = opts.apiKey;
    this.apiKeys = opts.apiKeys ?? {};
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

  async _request(
    method: string,
    path: string,
    headers: Record<string, string>,
    body?: unknown,
  ): Promise<Response> {
    return await this.fetchFn(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  }

  async listBooks(
    genre: string,
    cursor?: string,
    sort?: string,
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
    const res = await this._request("GET", path, headers);
    if (res.status < 200 || res.status >= 300) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    if (res.status === 200) {
      return (await res.json()) as models.ListBooksResponse;
    }
    throw new ApiError(res.status, await res.json().catch(() => null));
  }

  async createBook(body: models.BookDto): Promise<models.CreatedMessage> {
    let path = `/books/`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this._request("POST", path, headers, body);
    if (res.status < 200 || res.status >= 300) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    if (res.status === 201) {
      return (await res.json()) as models.CreatedMessage;
    }
    throw new ApiError(res.status, await res.json().catch(() => null));
  }

  async getBook(
    bookId: number,
    fmt?: models.BookFormat,
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
    const res = await this._request("GET", path, headers);
    if (res.status < 200 || res.status >= 300) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    if (res.status === 200) {
      return (await res.json()) as models.BookOrError;
    }
    throw new ApiError(res.status, await res.json().catch(() => null));
  }

  async updateBook(
    bookId: number,
    body: models.BookFilters,
  ): Promise<models.CreatedMessage> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this._request("PUT", path, headers, body);
    if (res.status < 200 || res.status >= 300) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    if (res.status === 200) {
      return (await res.json()) as models.CreatedMessage;
    }
    throw new ApiError(res.status, await res.json().catch(() => null));
  }
}
