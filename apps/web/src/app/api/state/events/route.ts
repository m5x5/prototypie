import { currentVersion, subscribe, type ChangeEvent } from "@/lib/server/state-store";

/**
 * Server-sent events: one `data: { epoch, version, clientId? }` message per state change (plus one on
 * connect). Clients then fetch GET /api/state?since=<their cursor>. Comment lines keep proxies from
 * closing an idle connection.
 */
export async function GET(request: Request) {
  const enc = new TextEncoder();
  const hello = await currentVersion();
  let cleanup = () => {};
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const send = (e: ChangeEvent) => controller.enqueue(enc.encode(`data: ${JSON.stringify(e)}\n\n`));
      send(hello);
      const unsub = subscribe(send);
      const ping = setInterval(() => controller.enqueue(enc.encode(": ping\n\n")), 20_000);
      cleanup = () => {
        unsub();
        clearInterval(ping);
        try {
          controller.close();
        } catch {
          // already closed
        }
      };
      request.signal.addEventListener("abort", () => cleanup());
    },
    cancel() {
      cleanup();
    },
  });
  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream; charset=utf-8",
      "Cache-Control": "no-store, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}
