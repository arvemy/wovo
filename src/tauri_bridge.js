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

export async function listen(event, handler) {
  const internals = tauriInternals();
  const eventId = await internals.invoke("plugin:event|listen", {
    event,
    target: { kind: "Any" },
    handler: internals.transformCallback(handler),
  });
  return eventId;
}
