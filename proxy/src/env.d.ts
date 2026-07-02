export {};

// GROQ_API_KEY is set via `wrangler secret put GROQ_API_KEY`, so it never shows up in
// `worker-configuration.d.ts` (generated from wrangler.jsonc vars/bindings only). Declaration
// merging into the global `Env` interface is the supported way to type secrets like this.
declare global {
	interface Env {
		GROQ_API_KEY: string;
	}
}
