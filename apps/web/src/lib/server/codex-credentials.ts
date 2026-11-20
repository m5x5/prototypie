import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import {
	InMemoryCredentialStore,
	type AuthOperationOptions,
	type Credential,
	type CredentialInfo,
	type CredentialStore,
	type OAuthCredential,
} from "@earendil-works/pi-ai";

/**
 * The Codex CLI login bridge: pi-ai's `openai-codex` provider authenticates
 * with a ChatGPT OAuth credential, and the Codex CLI already holds one in
 * $CODEX_HOME/auth.json (default ~/.codex/auth.json). This store serves that
 * file as the provider's credential — re-read on every access, so a login
 * or refresh done by the CLI is picked up live — and writes a pi-ai refresh
 * back in the CLI's own format, so the two never hold diverging (rotated)
 * refresh tokens. Every other provider falls through to an in-memory store,
 * i.e. pi-ai's usual env-var resolution.
 */

export const CODEX_PROVIDER_ID = "openai-codex";

/** The Codex CLI's auth file ($CODEX_HOME honoured, as the CLI does). */
export function codexAuthFile(): string {
	const home = process.env.CODEX_HOME || path.join(os.homedir(), ".codex");
	return path.join(home, "auth.json");
}

interface CodexAuthFile {
	auth_mode?: string;
	tokens?: {
		id_token?: string;
		access_token?: string;
		refresh_token?: string;
		account_id?: string;
	};
	last_refresh?: string;
	[key: string]: unknown;
}

function readCodexAuth(): CodexAuthFile | undefined {
	try {
		const parsed = JSON.parse(fs.readFileSync(codexAuthFile(), "utf8")) as CodexAuthFile;
		return parsed.tokens?.access_token && parsed.tokens.refresh_token ? parsed : undefined;
	} catch {
		return undefined;
	}
}

/** The JWT's `exp` in epoch ms; 0 (= refresh now) when unreadable. */
function jwtExpiryMs(token: string): number {
	try {
		const payload = JSON.parse(Buffer.from(token.split(".")[1] ?? "", "base64url").toString("utf8"));
		return typeof payload.exp === "number" ? payload.exp * 1000 : 0;
	} catch {
		return 0;
	}
}

function toCredential(file: CodexAuthFile): OAuthCredential {
	const tokens = file.tokens!;
	return {
		type: "oauth",
		access: tokens.access_token!,
		refresh: tokens.refresh_token!,
		expires: jwtExpiryMs(tokens.access_token!),
		...(tokens.account_id ? { accountId: tokens.account_id } : {}),
	};
}

/** Is a ChatGPT login present in the Codex CLI's auth file? */
export function codexLoggedIn(): boolean {
	return readCodexAuth() !== undefined;
}

/** Persist a refreshed credential in the CLI's format (other fields kept). */
function writeCodexAuth(credential: OAuthCredential): void {
	const file = codexAuthFile();
	const current = readCodexAuth() ?? {};
	const next: CodexAuthFile = {
		...current,
		tokens: {
			...current.tokens,
			access_token: credential.access,
			refresh_token: credential.refresh,
			...(typeof credential.accountId === "string" ? { account_id: credential.accountId } : {}),
		},
		last_refresh: new Date().toISOString(),
	};
	const tmp = `${file}.${process.pid}.tmp`;
	fs.writeFileSync(tmp, JSON.stringify(next, null, 2), { mode: 0o600 });
	fs.renameSync(tmp, file);
}

export class CodexCliCredentialStore implements CredentialStore {
	private readonly inner = new InMemoryCredentialStore();
	/** Serializes codex modifies in-process (pi-ai's refresh contract). */
	private chain: Promise<unknown> = Promise.resolve();

	async read(providerId: string, options?: AuthOperationOptions): Promise<Credential | undefined> {
		if (providerId !== CODEX_PROVIDER_ID) return this.inner.read(providerId, options);
		const file = readCodexAuth();
		return file ? toCredential(file) : undefined;
	}

	async list(options?: AuthOperationOptions): Promise<readonly CredentialInfo[]> {
		const rest = await this.inner.list(options);
		return codexLoggedIn() ? [...rest, { providerId: CODEX_PROVIDER_ID, type: "oauth" }] : rest;
	}

	modify(
		providerId: string,
		fn: (current: Credential | undefined) => Promise<Credential | undefined>,
		options?: AuthOperationOptions,
	): Promise<Credential | undefined> {
		if (providerId !== CODEX_PROVIDER_ID) return this.inner.modify(providerId, fn, options);
		const run = this.chain.then(async () => {
			const current = await this.read(providerId);
			const next = await fn(current);
			if (!next) return current;
			if (next.type === "oauth") writeCodexAuth(next);
			return next;
		});
		this.chain = run.catch(() => undefined);
		return run;
	}

	async delete(providerId: string, options?: AuthOperationOptions): Promise<void> {
		// Never log the user out of the Codex CLI from here.
		if (providerId !== CODEX_PROVIDER_ID) return this.inner.delete(providerId, options);
	}
}
