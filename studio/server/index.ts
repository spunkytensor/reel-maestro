import { createApp } from "./app.js";

const port = Number(process.env.REELMAESTRO_PORT || 3000);
const host = process.env.REELMAESTRO_HOST || "127.0.0.1";
const app = await createApp({ port, host });
let closing: Promise<void> | undefined;
const close = (): Promise<void> => {
  closing ??= app.close();
  return closing;
};
const shutdown = (): void => {
  void close().then(
    () => process.exit(0),
    () => process.exit(1),
  );
};
process.once("SIGINT", shutdown);
process.once("SIGTERM", shutdown);

try {
  await app.listen({ host, port });
  console.log(`Reel Maestro Studio listening at http://${host}:${port}`);
} catch (error) {
  await close();
  throw error;
}
