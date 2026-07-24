// NestJS bookstore controller — STATIC fixture source (Phase 1).
//
// Routing uses @nestjs/common's framework-native decorators (@Controller, @Get,
// @Post, @Put, @Param, @Query, @Body) — the direct Gin analog. Every
// request/response/param fact is derived from the method SIGNATURE + the typed
// DTO classes in `books.dto.ts`; nothing reads a third-party schema-annotation
// decorator or a runtime schema export (CLAUDE.md rule 1).
//
// The static @Controller('books') prefix is composed into the neutral graph
// operation paths (`/books/`, `/books/{bookId}`). No app runs this phase; this
// is the static source tsextract reads.
//
// PROVENANCE NOTE (non-fact prose only — rule 1): blank lines / comments below
// are SPACING ONLY, present so each method-name and param-name line anchors to
// the committed graph snapshot's asserted span. The snapshot is authoritative.
import {
  Body,
  Controller,
  Get,
  Param,
  Post,
  Put,
  Query,
} from '@nestjs/common';

import {
  BookDto,
  BookFilters,
  BookFormat,
  BookOrError,
  CreatedMessage,
  ListBooksResponse,
} from './books.dto';
//
// The comment/blank lines between handlers below are SPACING ONLY (rule 1): no
// API fact is encoded in any comment. They anchor each method-name and param-name
// line to the committed graph snapshot's asserted span. (spacing — non-fact)
@Controller('books')
export class BooksController {
  @Get('/')
  listBooks(
    @Query('genre') genre: string,
    @Query('sort') sort: string = 'asc',
    @Query('cursor') cursor?: string,
  ): ListBooksResponse {
    throw new Error('static fixture: never executed this phase');
  }
  // POST /books/ — a typed request body + a typed response. (spacing follows.)
  //
  @Post('/')
  createBook(@Body() book: BookDto): CreatedMessage {
    throw new Error('static fixture: never executed this phase');
  }
  // GET /books/:bookId — a path param + a UNION response. (spacing follows.)
  //
  @Get('/:bookId')
  getBook(
    @Param('bookId') bookId: number,
    @Query('fmt') fmt?: BookFormat,
  ): BookOrError {
    throw new Error('static fixture: never executed this phase');
  }
  // PUT /books/:bookId — path param + a body exercising all four axes.
  @Put('/:bookId')
  updateBook(
    @Param('bookId') bookId: number,
    @Body() filters: BookFilters,
  ): CreatedMessage {
    throw new Error('static fixture: never executed this phase');
  }
}
