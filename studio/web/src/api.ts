let csrfToken: string | undefined;
let session: Promise<void> | undefined;

async function connect() {
  session ??= fetch("/api/session", {
    credentials: "same-origin",
    cache: "no-store",
  })
    .then(async (response) => {
      if (!response.ok)
        throw new Error(
          "Studio could not connect. Check that the local server is running.",
        );
      const value: { csrfToken: string } = await response.json();
      csrfToken = value.csrfToken;
    })
    .catch((error: unknown) => {
      session = undefined;
      throw error;
    });
  return session;
}

export async function api<T>(url: string, body?: unknown): Promise<T> {
  await connect();
  const response = await fetch(`/api${url}`, {
    method: body === undefined ? "GET" : "POST",
    credentials: "same-origin",
    cache: "no-store",
    headers:
      body === undefined
        ? {}
        : {
            "Content-Type": "application/json",
            "X-CSRF-Token": csrfToken ?? "",
          },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (response.status === 401) {
    session = undefined;
    csrfToken = undefined;
    // Never silently replay a write after reconnecting.
    throw new Error("Studio restarted. Refresh this page before trying again.");
  }
  if (!response.ok) {
    const value: unknown = await response.json().catch(() => undefined);
    throw new Error(
      value &&
      typeof value === "object" &&
      "error" in value &&
      typeof value.error === "string"
        ? value.error
        : "That action could not be completed. Please try again.",
    );
  }
  return response.json() as Promise<T>;
}

export function errorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : "Something went wrong. Please try again.";
}

// Unlike randomUUID, getRandomValues also works on an explicitly enabled HTTP LAN listener.
export function approvalKey(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(24)), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}
