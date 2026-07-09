export interface ApiErrorInit {
  headers?: Headers;
  requestId?: string;
  rawBody?: string;
  jsonBody?: unknown;
  body?: unknown;
}

export class ApiError extends Error {
  public readonly headers: Headers;
  public readonly requestId?: string;
  public readonly rawBody: string;
  public readonly jsonBody: unknown;
  public readonly body: unknown;

  constructor(
    public readonly status: number,
    init: ApiErrorInit = {},
  ) {
    super(`HTTP ${status}`);
    this.name = "ApiError";
    this.headers = init.headers ?? new Headers();
    this.requestId = init.requestId;
    this.rawBody = init.rawBody ?? "";
    this.jsonBody = init.jsonBody ?? null;
    this.body = init.body ?? this.jsonBody;
  }

  isNotFound(): boolean {
    return this.status === 404;
  }
}
