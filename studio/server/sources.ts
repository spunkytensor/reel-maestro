import { lookup } from "node:dns/promises";
import http from "node:http";
import https from "node:https";
import { BlockList, isIP } from "node:net";
import type { UrlSource } from "../shared.js";

const blocked = new BlockList();
for (const [network, prefix] of [
  ["0.0.0.0", 8],
  ["10.0.0.0", 8],
  ["100.64.0.0", 10],
  ["127.0.0.0", 8],
  ["169.254.0.0", 16],
  ["172.16.0.0", 12],
  ["192.0.0.0", 24],
  ["192.168.0.0", 16],
  ["198.18.0.0", 15],
  ["224.0.0.0", 4],
  ["240.0.0.0", 4],
] as const)
  blocked.addSubnet(network, prefix, "ipv4");
for (const [network, prefix] of [
  ["::", 128],
  ["::1", 128],
  ["fc00::", 7],
  ["fe80::", 10],
  ["ff00::", 8],
  ["2001:db8::", 32],
] as const)
  blocked.addSubnet(network, prefix, "ipv6");

export async function fetchUrlSource(
  source: string,
): Promise<UrlSource | string> {
  return requestUrl(source, 0);
}

async function requestUrl(
  source: string,
  redirects: number,
): Promise<UrlSource | string> {
  let url: URL;
  try {
    url = new URL(source);
  } catch {
    return "Invalid URL";
  }
  if (
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username ||
    url.password ||
    !url.hostname ||
    (url.port && url.port !== "80" && url.port !== "443")
  )
    return "URL is not permitted";
  const answers = await lookup(url.hostname, {
    all: true,
    verbatim: true,
  }).catch(() => []);
  if (
    !answers.length ||
    answers.some((answer) => !publicAddress(answer.address))
  )
    return "URL address is not permitted";
  const pinned = answers[0];
  return new Promise((resolve) => {
    const transport = url.protocol === "https:" ? https : http;
    const request = transport.get(
      url,
      {
        headers: {
          Accept: "text/html,text/plain",
          "User-Agent": "ReelMaestro-Studio/1",
        },
        lookup: (_hostname, _options, callback) =>
          callback(null, pinned.address, pinned.family),
        signal: AbortSignal.timeout(10_000),
      },
      (response) => {
        const status = response.statusCode ?? 0;
        if (status >= 300 && status < 400 && response.headers.location) {
          response.resume();
          if (redirects >= 4) return resolve("Too many redirects");
          return void requestUrl(
            new URL(response.headers.location, url).href,
            redirects + 1,
          ).then(resolve);
        }
        if (status < 200 || status >= 300) {
          response.resume();
          return resolve("URL could not be fetched");
        }
        const type = String(response.headers["content-type"] ?? "")
          .split(";", 1)[0]
          .trim()
          .toLowerCase();
        if (type !== "text/html" && type !== "text/plain") {
          response.resume();
          return resolve("URL content type is not supported");
        }
        if (response.headers["content-encoding"]) {
          response.resume();
          return resolve("Encoded URL responses are not supported");
        }
        const declared = Number(response.headers["content-length"]);
        if (Number.isFinite(declared) && declared > 2_000_000) {
          response.resume();
          return resolve("URL response is too large");
        }
        const chunks: Buffer[] = [];
        let size = 0;
        let settled = false;
        const finish = (value: UrlSource | string): void => {
          if (!settled) {
            settled = true;
            resolve(value);
          }
        };
        response.on("data", (chunk: Buffer) => {
          size += chunk.length;
          if (size > 2_000_000) {
            response.destroy();
            finish("URL response is too large");
          } else chunks.push(chunk);
        });
        response.once("error", () => finish("URL could not be fetched"));
        response.once("end", () => {
          if (settled) return;
          const raw = Buffer.concat(chunks).toString("utf8");
          const titleMatch =
            type === "text/html"
              ? /<title\b[^>]*>([\s\S]*?)<\/title>/i.exec(raw)
              : undefined;
          const text = (type === "text/html" ? htmlText(raw) : raw)
            .trim()
            .slice(0, 500_000);
          if (!text) return finish("URL did not contain readable text");
          const title = titleMatch
            ? decodeEntities(titleMatch[1].replace(/<[^>]*>/g, " "))
                .replace(/\s+/g, " ")
                .trim()
                .slice(0, 500)
            : "";
          finish({ text, ...(title ? { title } : {}), url: url.href });
        });
      },
    );
    request.once("error", () => resolve("URL could not be fetched"));
  });
}

function publicAddress(address: string): boolean {
  const family = isIP(address);
  if (!family) return false;
  if (family === 6 && /^::ffff:/i.test(address))
    return publicAddress(address.slice(7));
  return !blocked.check(address, family === 4 ? "ipv4" : "ipv6");
}
function htmlText(source: string): string {
  return decodeEntities(
    source
      .replace(
        /<(?:script|style|noscript|svg)\b[^>]*>[\s\S]*?<\/(?:script|style|noscript|svg)>/gi,
        " ",
      )
      .replace(/<[^>]+>/g, " ")
      .replace(/\s+/g, " "),
  );
}
function decodeEntities(value: string): string {
  return value.replace(
    /&(amp|lt|gt|quot|#39|nbsp);/gi,
    (match, name: string) =>
      ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", nbsp: " " })[
        name.toLowerCase()
      ] ?? match,
  );
}
