import { Client, type HookContext } from "./index";

type CapturedMultipart = {
  fileContents: string[];
  request: string | null;
};

const multipart: CapturedMultipart[] = [];
const redirectModes: RequestRedirect[] = [];
const responses: HookContext[] = [];
const searchQueries: string[] = [];
let manualRedirects = 0;

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

const transport: typeof fetch = async (input, init) => {
  const path = new URL(String(input)).pathname;
  if (path === "/v1/files/upload") {
    if (init?.body instanceof FormData) {
      check(
        !new Headers(init.headers).has("Content-Type"),
        "multipart boundary header must be transport-owned",
      );
      const fileContents: string[] = [];
      for (const value of init.body.getAll("files")) {
        if (!(value instanceof Blob)) throw new Error("file part must be a Blob");
        fileContents.push(await value.text());
      }
      multipart.push({
        fileContents,
        request: init.body.get("request") as string | null,
      });
    } else {
      check(
        new Headers(init?.headers).get("Content-Type") === "application/json",
        "JSON Content-Type missing",
      );
      check(init?.body === '{"title":"json"}', `JSON body=${String(init?.body)}`);
    }
    return new Response(null, { status: 204 });
  }
  if (path === "/v1/items/search") {
    searchQueries.push(new URL(String(input)).search);
    return new Response(
      JSON.stringify({
        q: "",
        limit: 0,
        offset: 0,
        page: 1,
        days: 0,
        sort: "",
        cursor: "",
        token: "",
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
  }
  if (path.endsWith("/redirect")) {
    const mode = init?.redirect ?? "follow";
    redirectModes.push(mode);
    if (mode === "follow") return new Response(null, { status: 204 });
    manualRedirects += 1;
    if (manualRedirects > 1) {
      return {
        type: "opaqueredirect",
        status: 0,
        headers: new Headers(),
        body: null,
      } as unknown as Response;
    }
    return new Response(null, {
      status: 307,
      headers: { Location: "/v1/files/final", "X-Session-ID": "session-123" },
    });
  }
  throw new Error(`unexpected path: ${path}`);
};

async function main(): Promise<void> {
  const client = new Client({
    baseUrl: "https://api.test",
    fetch: transport,
    hooks: {
      response: [
        (context) => {
          responses.push({ ...context });
        },
      ],
    },
  });
  await client.uploadFile({ contentType: "application/json", value: { title: "json" } });
  await client.uploadFile({
    contentType: "multipart/form-data",
    value: { request: '{"name":"multipart"}' },
  });
  await client.uploadFile({
    contentType: "multipart/form-data",
    value: { request: '{"name":"multipart"}', files: [new Uint8Array([111, 110, 101])] },
  });
  await client.uploadFile({
    contentType: "multipart/form-data",
    value: {
      request: '{"name":"multipart"}',
      files: [
        new Uint8Array([111, 110, 101]),
        new Uint8Array([116, 119, 111]),
      ],
    },
  });
  check(multipart.length === 3, `multipart calls=${multipart.length}`);
  check(
    multipart[0].fileContents.length === 0 &&
      multipart[0].request === '{"name":"multipart"}',
    "optional files were not omitted",
  );
  check(multipart[1].fileContents.join(",") === "one", `one file=${multipart[1].fileContents}`);
  check(multipart[2].fileContents.join(",") === "one,two", `many files=${multipart[2].fileContents}`);
  check(multipart[2].request === '{"name":"multipart"}', `request part=${multipart[2].request}`);

  await client.searchItems({ page: 1, q: "" });
  await client.searchItems({ page: 1, q: "", offset: 0 });
  check(!searchQueries[0].includes("offset="), `absent offset=${searchQueries[0]}`);
  check(!searchQueries[0].includes("sort=") && !searchQueries[0].includes("cursor="), `defaults leaked=${searchQueries[0]}`);
  check(searchQueries[1].includes("offset=0"), `explicit zero offset=${searchQueries[1]}`);

  await client.redirectFile("file-1");
  await client.redirectFile("file-1");
  await client.redirectFile("file-1", { followRedirects: true });
  check(
    redirectModes.join(",") === "manual,manual,follow",
    `redirect modes=${redirectModes}`,
  );
  const redirectResponses = responses.filter((context) => context.operationId === "redirectFile");
  check(redirectResponses.length === 3, `redirect hooks=${redirectResponses.length}`);
  check(redirectResponses[0]?.status === 307, "server redirect status was not exposed");
  check(
    redirectResponses[0]?.responseHeaders?.get("X-Session-ID") === "session-123",
    "server redirect headers were not exposed",
  );
  check(
    redirectResponses[1]?.status === 0 && redirectResponses[1]?.opaqueRedirect === true,
    "browser opaque redirect was not reported as an accepted opaque outcome",
  );
  check(
    redirectResponses[1]?.responseHeaders?.get("Location") === null,
    "browser opaque redirect fabricated a Location header",
  );
  check(
    redirectResponses[2]?.status === 204 && redirectResponses[2]?.opaqueRedirect === false,
    "followed redirect did not expose the final response",
  );
}

void main().catch((error: unknown) => {
  console.error(error);
  throw error;
});
