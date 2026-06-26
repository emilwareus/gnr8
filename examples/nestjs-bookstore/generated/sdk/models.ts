export interface AuthorDto {
  bio: string | null;
  name: string;
}

export interface BookDto {
  author: AuthorDto;
  format: BookFormat;
  id: number;
  rating?: number | null;
  tags?: string[];
  title: string;
}

export interface BookFilters {
  genre: string;
  inStock?: boolean;
  published: number | null;
  sort?: "asc" | "desc" | null;
}

export type BookFormat = "hardcover" | "paperback";

export type BookOrError = BookDto | OutOfStockDto;

export interface CreatedMessage {
  id: number;
  message: string;
}

export interface ListBooksResponse {
  books: BookDto[];
  nextCursor: string | null;
  total: number;
}

export interface OutOfStockDto {
  reason: string;
}
