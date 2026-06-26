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
    const query = new URLSearchParams();
    query.set("genre", String(genre));
    if (cursor !== undefined) {
      query.set("cursor", String(cursor));
    }
    if (sort !== undefined) {
      query.set("sort", String(sort));
    }
    const qs = query.toString();
    if (qs) {
      path = path + "?" + qs;
    }
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "GET",
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.ListBooksResponse;
  }


  async createBook(body: models.BookDto): Promise<models.CreatedMessage> {
    let path = `/books/`;
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (res.status !== 201) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.CreatedMessage;
  }


  async getBook(bookId: number, fmt?: models.BookFormat): Promise<models.BookOrError> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const query = new URLSearchParams();
    if (fmt !== undefined) {
      query.set("fmt", String(fmt));
    }
    const qs = query.toString();
    if (qs) {
      path = path + "?" + qs;
    }
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "GET",
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.BookOrError;
  }


  async updateBook(bookId: number, body: models.BookFilters): Promise<models.CreatedMessage> {
    let path = `/books/${encodeURIComponent(String(bookId))}`;
    const res = await this.fetchFn(`${this.baseUrl}${path}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (res.status !== 200) {
      throw new ApiError(res.status, await res.json().catch(() => null));
    }
    return (await res.json()) as models.CreatedMessage;
  }
}
