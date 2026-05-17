function tauriInternals() {
  const internals = window.__TAURI_INTERNALS__;
  if (!internals) {
    throw new Error("Tauri internals are unavailable.");
  }
  return internals;
}

export function invoke(command, args) {
  return tauriInternals().invoke(command, args);
}

export async function invokeWithPolicy(command, args, timeoutMs, retries, retryDelayMs) {
  let attempt = 0;
  while (true) {
    try {
      return await invokeWithTimeout(command, args, timeoutMs);
    } catch (error) {
      if (attempt >= retries) {
        throw error;
      }
      attempt += 1;
      await delay(retryDelayMs * attempt);
    }
  }
}

function invokeWithTimeout(command, args, timeoutMs) {
  const request = invoke(command, args);
  if (!timeoutMs) {
    return request;
  }

  let timeoutId;
  const timeout = new Promise((_, reject) => {
    timeoutId = window.setTimeout(() => {
      reject({
        code: "ipc_timeout",
        userMessage: "Wovo could not complete the request in time.",
        message: "Wovo could not complete the request in time.",
      });
    }, timeoutMs);
  });

  return Promise.race([request, timeout]).finally(() => {
    window.clearTimeout(timeoutId);
  });
}

function delay(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export async function listen(event, handler) {
  const internals = tauriInternals();
  const eventId = await internals.invoke("plugin:event|listen", {
    event,
    target: { kind: "Any" },
    handler: internals.transformCallback(handler),
  });
  return eventId;
}
