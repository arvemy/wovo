import { invoke as tauriInvoke } from "/tauri-api/core.js";
import { listen as tauriListen } from "/tauri-api/event.js";

export function invoke(command, args) {
  return tauriInvoke(command, args);
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
  return await tauriListen(event, handler);
}
