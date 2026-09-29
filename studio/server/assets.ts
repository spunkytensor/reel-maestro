import {
  createHash,
  createHmac,
  randomBytes,
  timingSafeEqual,
} from "node:crypto";
import { lstat, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import type { UploadAsset } from "../shared.js";

type AssetMetadata = UploadAsset & { sha256: string };
const LIMITS = { text: 1_000_000, image: 15_000_000, audio: 50_000_000 };

export class AssetStore {
  private readonly directory: string;
  private readonly secretFile: string;
  private secret?: Buffer;

  constructor(stateDir: string) {
    this.directory = path.join(stateDir, "uploads");
    this.secretFile = path.join(stateDir, "asset-signing-key");
  }

  async create(value: unknown): Promise<UploadAsset | string> {
    const body = object(value);
    if (
      !body ||
      Object.keys(body).some((key) => !["name", "kind", "data"].includes(key))
    )
      return "Invalid upload";
    if (
      typeof body.name !== "string" ||
      !body.name.trim() ||
      body.name.length > 255 ||
      /[\u0000\r\n]/.test(body.name)
    )
      return "Invalid upload name";
    if (body.kind !== "text" && body.kind !== "image" && body.kind !== "audio")
      return "Invalid upload kind";
    if (typeof body.data !== "string" || !strictBase64(body.data))
      return "Invalid base64 data";
    const bytes = Buffer.from(body.data, "base64");
    if (bytes.length === 0 || bytes.length > LIMITS[body.kind])
      return "Upload size is invalid";
    const detected = detect(body.kind, bytes);
    if (!detected) return `Invalid ${body.kind} content`;
    const id = randomBytes(24).toString("base64url");
    const metadata: AssetMetadata = {
      id: await this.sign(id),
      name: body.name.trim(),
      kind: body.kind,
      size: bytes.length,
      mime: detected.mime,
      ...(detected.text === undefined ? {} : { text: detected.text }),
      sha256: createHash("sha256").update(bytes).digest("hex"),
    };
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    await atomicWrite(path.join(this.directory, `${id}.bin`), bytes);
    await atomicWrite(
      path.join(this.directory, `${id}.json`),
      Buffer.from(JSON.stringify(metadata)),
    );
    const { sha256: _sha256, ...asset } = metadata;
    return asset;
  }

  async resolve(
    assetId: string,
    expected?: UploadAsset["kind"],
  ): Promise<{ path: string; metadata: UploadAsset } | undefined> {
    const id = await this.verify(assetId);
    if (!id) return undefined;
    const dataFile = path.join(this.directory, `${id}.bin`);
    const metadataFile = path.join(this.directory, `${id}.json`);
    const [dataInfo, metadataInfo] = await Promise.all([
      lstat(dataFile).catch(() => undefined),
      lstat(metadataFile).catch(() => undefined),
    ]);
    if (
      !dataInfo?.isFile() ||
      dataInfo.isSymbolicLink() ||
      !metadataInfo?.isFile() ||
      metadataInfo.isSymbolicLink()
    )
      return undefined;
    const parsed = parseMetadata(
      await readFile(metadataFile, "utf8").catch(() => ""),
    );
    if (
      !parsed ||
      parsed.id !== assetId ||
      (expected && parsed.kind !== expected) ||
      parsed.size !== dataInfo.size
    )
      return undefined;
    const digest = createHash("sha256")
      .update(await readFile(dataFile))
      .digest("hex");
    if (digest !== parsed.sha256) return undefined;
    const { sha256: _sha256, ...metadata } = parsed;
    return { path: dataFile, metadata };
  }

  private async key(): Promise<Buffer> {
    if (this.secret) return this.secret;
    await mkdir(path.dirname(this.secretFile), {
      recursive: true,
      mode: 0o700,
    });
    try {
      const existing = await readFile(this.secretFile);
      if (existing.length !== 32) throw new Error("invalid signing key");
      this.secret = existing;
    } catch (error) {
      if (code(error) !== "ENOENT") throw error;
      const secret = randomBytes(32);
      try {
        await writeFile(this.secretFile, secret, { flag: "wx", mode: 0o600 });
        this.secret = secret;
      } catch (writeError) {
        if (code(writeError) !== "EEXIST") throw writeError;
        this.secret = await readFile(this.secretFile);
      }
    }
    return this.secret;
  }
  private async sign(id: string): Promise<string> {
    const signature = createHmac("sha256", await this.key())
      .update(id)
      .digest("base64url");
    return `${id}.${signature}`;
  }
  private async verify(assetId: string): Promise<string | undefined> {
    const match = /^([A-Za-z0-9_-]{32})\.([A-Za-z0-9_-]{43})$/.exec(assetId);
    if (!match) return undefined;
    const expected = await this.sign(match[1]);
    const left = Buffer.from(assetId);
    const right = Buffer.from(expected);
    return left.length === right.length && timingSafeEqual(left, right)
      ? match[1]
      : undefined;
  }
}

async function atomicWrite(file: string, data: Buffer): Promise<void> {
  const temporary = `${file}.${randomBytes(8).toString("hex")}.tmp`;
  await writeFile(temporary, data, { mode: 0o600 });
  await rename(temporary, file);
}
function strictBase64(value: string): boolean {
  return (
    value.length <= Math.ceil(LIMITS.audio / 3) * 4 + 4 &&
    value.length % 4 === 0 &&
    /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(
      value,
    )
  );
}
function detect(
  kind: UploadAsset["kind"],
  bytes: Buffer,
): { mime: string; text?: string } | undefined {
  if (kind === "text") {
    const text = bytes.toString("utf8");
    if (text.includes("\uFFFD") || text.includes("\u0000")) return undefined;
    return { mime: "text/plain; charset=utf-8", text };
  }
  if (kind === "image") {
    if (bytes.subarray(0, 3).equals(Buffer.from([0xff, 0xd8, 0xff])))
      return { mime: "image/jpeg" };
    if (bytes.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex")))
      return { mime: "image/png" };
    if (
      bytes.subarray(0, 4).toString() === "RIFF" &&
      bytes.subarray(8, 12).toString() === "WEBP"
    )
      return { mime: "image/webp" };
    return undefined;
  }
  if (
    bytes.subarray(0, 3).toString() === "ID3" ||
    (bytes[0] === 0xff && (bytes[1] & 0xe0) === 0xe0)
  )
    return { mime: "audio/mpeg" };
  if (
    bytes.subarray(0, 4).toString() === "RIFF" &&
    bytes.subarray(8, 12).toString() === "WAVE"
  )
    return { mime: "audio/wav" };
  if (bytes.length >= 12 && bytes.subarray(4, 8).toString() === "ftyp")
    return { mime: "audio/mp4" };
  return undefined;
}
function parseMetadata(source: string): AssetMetadata | undefined {
  try {
    const value = object(JSON.parse(source));
    if (
      !value ||
      typeof value.id !== "string" ||
      typeof value.name !== "string" ||
      (value.kind !== "text" &&
        value.kind !== "image" &&
        value.kind !== "audio") ||
      typeof value.size !== "number" ||
      !Number.isSafeInteger(value.size) ||
      typeof value.mime !== "string" ||
      typeof value.sha256 !== "string" ||
      !/^[a-f0-9]{64}$/.test(value.sha256) ||
      (value.text !== undefined && typeof value.text !== "string")
    )
      return undefined;
    return {
      id: value.id,
      name: value.name,
      kind: value.kind,
      size: value.size,
      mime: value.mime,
      ...(typeof value.text === "string" ? { text: value.text } : {}),
      sha256: value.sha256,
    };
  } catch {
    return undefined;
  }
}
function object(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : undefined;
}
function code(error: unknown): unknown {
  return error !== null && typeof error === "object" && "code" in error
    ? error.code
    : undefined;
}
