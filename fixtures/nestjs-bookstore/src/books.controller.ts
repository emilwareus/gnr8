// NestJS bookstore controller — STATIC fixture source (Phase 1).
//
// Routing uses @nestjs/common's framework-native decorators (@Controller, @Get,
// @Post, @Put, @Param, @Query, @Body) — the direct Gin analog. Every
// request/response/param fact is derived from the method SIGNATURE + the typed
// DTO classes in `books.dto.ts`. Nothing reads a third-party schema-annotation
// decorator or a runtime schema export; facts come from the source's own TS
// property types (CLAUDE.md rule 1).
//
// The @Controller('books') prefix is a lowering-time base path (rule 1): the
// neutral graph operation paths are group-relative (`/`, `/{bookId}`). No app
// runs this phase (no npm install); this is the static source tsextract reads.

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

@Controller('books')
export class BooksController {
  // GET /books/ — typed query params + a typed response envelope.
  //   genre  : required query string
  //   sort   : optional query string (has a default)
  //   cursor : optional query string (`?`)
  @Get('/')
  listBooks(
    @Query('genre') genre: string,
    @Query('sort') sort: string = 'asc',
    @Query('cursor') cursor?: string,
  ): ListBooksResponse {
    throw new Error('static fixture: never executed this phase');
  }

  // POST /books/ — a typed request body + a typed response.
  @Post('/')
  createBook(@Body() book: BookDto): CreatedMessage {
    throw new Error('static fixture: never executed this phase');
  }

  // GET /books/:bookId — a path param + a UNION response.
  //   bookId : required path number
  //   fmt    : optional query enum (string-literal union, `?`)
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
