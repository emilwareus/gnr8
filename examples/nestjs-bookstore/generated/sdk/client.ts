import { ApiError } from "./errors";
import * as models from "./models";

export interface ClientOptions {
  baseUrl: string;
  fetch?: typeof fetch;
}

export class Client {
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;


  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.fetchFn = opts.fetch ?? fetch;
  }


  async listBooks(genre: string, cursor?: string, sort?: string): Promise<models.ListBooksResponse> {
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
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "GET",
      headers,
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.ListBooksResponse;
  }


  async createBook(body: models.BookDto): Promise<models.CreatedMessage> {
    let path = `/books/`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });
    if (res.status !== 201) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.CreatedMessage;
  }


  async getBook(bookId: number, fmt?: models.BookFormat): Promise<models.BookOrError> {
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
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "GET",
      headers,
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.BookOrError;
  }


  async updateBook(bookId: number, body: models.BookFilters): Promise<models.CreatedMessage> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const headers: Record<string, string> = {};
    headers["Content-Type"] = "application/json";
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "PUT",
      headers,
      body: JSON.stringify(body),
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.CreatedMessage;
  }
}
