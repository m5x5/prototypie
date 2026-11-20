import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { createModels, type Tool } from "@earendil-works/pi-ai";
import { openaiCodexProvider } from "@earendil-works/pi-ai/providers/openai-codex";
import { z } from "zod";
import { CODEX_PROVIDER_ID, CodexCliCredentialStore, codexLoggedIn } from "./codex-credentials";

/**
 * The one LLM seam for every server route:
 * pi-ai's `openai-codex` provider authenticated by the Codex CLI's ChatGPT
 * login (~/.codex/auth.json), so no API key is needed - just `codex login`.
 *
 * Model: $PROTOTYPIE_MODEL, else the `model` from ~/.codex/config.toml, else gpt-6-astra.
 */

export type Reasoning = "minimal" | "low" | "medium" | "high" | "xhigh";

let models: ReturnType<typeof createModels> | undefined;
function collection() {
  if (!models) {
    models = createModels({
      identity: { userAgent: { custom: "prototypie/0.1" }, attribution: { name: "prototypie", url: "http://localhost:3123" } },
      credentials: new CodexCliCredentialStore(),
    });
    models.setProvider(openaiCodexProvider());
  }
  return models;
}

function configuredModelId(): string {
  if (process.env.PROTOTYPIE_MODEL) return process.env.PROTOTYPIE_MODEL;
  try {
    const home = process.env.CODEX_HOME || path.join(os.homedir(), ".codex");
    const toml = fs.readFileSync(path.join(home, "config.toml"), "utf8");
    const m = toml.match(/^\s*model\s*=\s*"([^"]+)"/m);
    if (m) return m[1];
  } catch {}
  return "gpt-6-astra";
}

export class LLMError extends Error {}

export function llmStatus() {
  return { provider: CODEX_PROVIDER_ID, model: configuredModelId(), loggedIn: codexLoggedIn() };
}

/**
 * Ask the model for one JSON object matching `schema`. Implemented as a single
 * strict "submit" tool call (pi-ai's provider-neutral structured output), then
 * validated with zod. Retries once if the model answers in prose instead.
 */
export async function generateObject<S extends z.ZodType>(opts: {
  system: string;
  prompt: string;
  schema: S;
  /** What the submit tool is for, e.g. "Submit the finished design". */
  description?: string;
  reasoning?: Reasoning;
  signal?: AbortSignal;
}): Promise<z.infer<S>> {
  if (!codexLoggedIn()) {
    throw new LLMError("Not logged in to Codex. Run `codex login` in a terminal (ChatGPT account), then retry.");
  }
  const m = collection();
  const id = configuredModelId();
  const model = m.getModel(CODEX_PROVIDER_ID, id);
  if (!model) throw new LLMError(`openai-codex has no model "${id}". Set PROTOTYPIE_MODEL.`);

  const tool: Tool = {
    name: "submit",
    description: opts.description ?? "Submit the final result.",
    // pi-ai takes TypeBox schemas, which are plain JSON Schema.
    parameters: z.toJSONSchema(opts.schema, { target: "draft-7" }) as Tool["parameters"],
    constrainedSampling: { type: "json_schema", strict: "prefer" },
  };

  let prompt = opts.prompt;
  for (let attempt = 0; attempt < 2; attempt++) {
    const res = await m.completeSimple(
      model,
      {
        systemPrompt: `${opts.system}\n\nAlways deliver your answer by calling the \`submit\` tool exactly once. Do not answer in plain text.`,
        messages: [{ role: "user", content: prompt, timestamp: Date.now() }],
        tools: [tool],
      },
      { reasoning: opts.reasoning ?? "low", signal: opts.signal },
    );
    if (res.stopReason === "error" || res.stopReason === "aborted") {
      throw new LLMError(res.errorMessage || `Model request ${res.stopReason}`);
    }
    const call = res.content.find((b) => b.type === "toolCall" && b.name === "submit");
    if (call && call.type === "toolCall") {
      const parsed = opts.schema.safeParse(call.arguments);
      if (parsed.success) return parsed.data;
      prompt = `${opts.prompt}\n\nYour previous submit call was invalid: ${parsed.error.message}. Call submit again with valid arguments.`;
      continue;
    }
    if (res.stopReason === "length") throw new LLMError("Output was cut off (too long).");
    prompt = `${opts.prompt}\n\nReminder: you must call the submit tool.`;
  }
  throw new LLMError("The model did not return a valid result.");
}

export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  return String(err);
}
