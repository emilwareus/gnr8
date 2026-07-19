// Controller edge cases locking the 04 warning fixes (WR-01..WR-05). These are
// NestJS routing shapes the acceptance fixture does not exercise; each must
// either map through the SINGLE type discriminator or diagnose-and-omit (rule 3),
// never silently drop a route/body or emit an out-of-range status.

import {
  Controller,
  Get,
  Post,
  Body,
  HttpCode,
} from "@nestjs/common";

export class Thing {
  id: number;
}

export type ThingOrError = Thing | Failure;

export class Failure {
  message: string;
}

@Controller("edges")
export class EdgesController {
  // WR-01: a nullable named return drops its aliasSymbol in TS; routed through
  // the single mapType path it resolves to the inline union residual, which is
  // not a TypeRef -> diagnosed + body omitted (not silently mis-mapped).
  @Get("/nullable")
  getNullable(): ThingOrError | null {
    return null;
  }

  // WR-02: an array return is representable as a type but not as a TypeRef ->
  // a DISTINCT "array/not-a-named-ref" diagnostic + body omitted.
  @Get("/array")
  getArray(): Thing[] {
    return [];
  }

  // WR-01 happy path: a plain named return still resolves to its ref_id via the
  // single path (the dual `t.aliasSymbol` discriminator is gone).
  @Get("/named")
  getNamed(): Thing {
    return new Thing();
  }

  // WR-03: a second HTTP-verb decorator must be diagnosed, not silently dropped.
  @Get("/multi")
  @Post("/multi")
  multiVerb(): void {}

  // WR-04: a second @Body must be diagnosed, not silently first-wins.
  // WR-05: an out-of-range @HttpCode must be diagnosed + the override ignored.
  @Post("/bad")
  @HttpCode(700)
  bad(@Body() a: Thing, @Body() b: Thing): void {}
}

const DYNAMIC_PREFIX = "dynamic";

@Controller(DYNAMIC_PREFIX)
export class DynamicPrefixController {
  @Get("/")
  omitted(): Thing {
    return new Thing();
  }
}
