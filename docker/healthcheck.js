import http from "node:http";

const port = Number(process.env.REELMAESTRO_PORT || 3000);
const path = process.env.REELMAESTRO_READINESS_PATH || "/api/ready";
const request = http.get(
  {
    host: "127.0.0.1",
    port,
    path,
    headers: { Host: `localhost:${port}` },
    timeout: 8_000,
  },
  (response) => {
    response.resume();
    response.once("end", () => process.exit(response.statusCode === 200 ? 0 : 1));
  },
);
request.once("timeout", () => request.destroy(new Error("readiness timed out")));
request.once("error", () => process.exit(1));
