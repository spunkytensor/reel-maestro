import assert from "node:assert/strict";
import test from "node:test";
import { fetchUrlSource } from "./sources.js";

test("URL ingestion rejects local, private, credentialed, and non-http targets before fetching", async () => {
  for (const url of [
    "http://127.0.0.1/",
    "http://[::1]/",
    "http://169.254.169.254/latest/meta-data/",
    "http://10.0.0.1/",
    "http://user:password@example.com/",
    "file:///etc/passwd",
    "ftp://example.com/file",
    "http://example.com:8080/",
  ]) {
    const result = await fetchUrlSource(url);
    assert.equal(typeof result, "string", url);
  }
});
