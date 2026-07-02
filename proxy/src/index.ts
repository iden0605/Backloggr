/**
 * Thin proxy between the app and Groq's chat completions API. Keeps GROQ_API_KEY server-side
 * and normalizes the model's reply into a strict contract the Rust side can pattern-match on
 * without ever needing to parse free-form model prose.
 *
 * Both endpoints ask Groq to name specific games it knows (using its own gaming knowledge)
 * rather than a generic keyword string — a keyword search against RAWG's title index tends to
 * surface shovelware clones (e.g. "farming simulation" -> a wall of "Farming Simulator N"
 * reskins) instead of genuinely similar games. The Rust side looks each named title up on RAWG
 * individually, so results come out with real variety.
 */

interface ChatMessage {
	role: "user" | "assistant";
	content: string;
}

interface ChatRequestBody {
	message: string;
	history: ChatMessage[];
	// Set by the frontend, not inferred from history — it already knows whether this message is
	// answering a clarifying question it just displayed, or the user starting a new ask (which
	// happens right after a results turn, or turn one). Explicit beats inferred: history keeps
	// accumulating across the whole conversation (so later rounds have full context to deepen
	// the search), which would otherwise make "is this fresh" ambiguous to guess from length.
	awaitingAnswer: boolean;
}

interface FavoriteGame {
	name: string;
	genre: string | null;
}

interface SuggestRequestBody {
	games: FavoriteGame[];
}

// The chat flow is a deterministic two-step exchange enforced in code (see `handleChat`), not
// left to the model to decide — an earlier version asked Groq to judge "should I search or
// clarify" from prose instructions, and it frequently searched immediately anyway. Now the
// frontend tells the endpoint which mode this turn is (`awaitingAnswer`), so there's no
// ambiguity for the model to get wrong: every new ask gets exactly one clarifying question
// first (this prompt), then the immediate next message always searches (`SEARCH_SYSTEM_PROMPT`).
// The conversation history keeps accumulating across the whole session (never reset), so a user
// can keep chatting afterward to refine or go deeper — each new ask still gets its own
// clarifying question, but with the full prior conversation as context.
const CLARIFY_SYSTEM_PROMPT = `A player just asked for a game recommendation (see the conversation so far for any earlier context in this session). Your ONLY job is to ask exactly one clarifying question. Do not suggest any games yet.

First, mentally list what the request ALREADY tells you (genre, named game, mechanics, mood, setting, etc.) — then ask about something that is genuinely still unknown, not something already answered by the request itself. Never ask a question whose answer the player already gave you (e.g. don't ask singleplayer-vs-multiplayer if they already said "MMORPG"; don't ask "what genre" if they already named one).

Possible axes to ask about — pick whichever ONE is most useful given what's still missing (do not default to the same axis every time):
- Setting/theme (fantasy, sci-fi, modern, historical, post-apocalyptic...)
- Tone (lighthearted/wholesome vs. dark/gritty, serious vs. goofy)
- Pacing/commitment (short quick sessions vs. a long game to sink hours into)
- Difficulty/challenge level
- Social angle (solo-friendly, small co-op, or big group content) — only if not already implied
- Art style/perspective (pixel art, realistic, top-down, first-person...)
- A specific mechanic or feature they most want to revolve around
- What they enjoyed most about a game they named, if the request centers on a specific game rather than a genre

Reply with ONLY strict JSON, no prose, no markdown fences, in this exact shape:
{"question": "<one short clarifying question>", "options": ["<short choice>", "..."] or null, "multiSelect": <true or false>}

Set "multiSelect": true when more than one option could reasonably apply at once (e.g. tone, things they enjoy); set it false for a single either/or choice (e.g. difficulty level). Include "options" (2-5 short answers) whenever the question has natural discrete choices; use "options": null only for a genuinely open-ended question.`;

const SEARCH_SYSTEM_PROMPT = `A player asked for a game recommendation, you asked one clarifying question, and they've now answered it (see the conversation so far). Using their original request plus their answer, name 4 to 8 SPECIFIC, DISTINCT real games you know of that fit — never the same game or a reskin/sequel/clone of it repeated with minor name variations. Prioritize variety across different developers/series.

Reply with ONLY strict JSON, no prose, no markdown fences, in this exact shape:
{"titles": ["<specific real game title>", "..."], "reasoning": "<one short sentence on why these fit>"}`;

const SUGGEST_SYSTEM_PROMPT = `A player's most-played/enjoyed games are given to you. Suggest 4 to 8 SPECIFIC, DISTINCT real games they might also enjoy, based on genre and style — never the same game, a reskin, a sequel, or a near-duplicate title repeated with minor variations. Prioritize variety across different developers/series while still matching the player's taste.

Reply with ONLY strict JSON, no prose, no markdown fences, in this exact shape:
{"titles": ["<specific real game title>", "..."], "reasoning": "<one short sentence on why these fit>"}`;

const CORS_HEADERS = {
	"Access-Control-Allow-Origin": "*",
	"Access-Control-Allow-Methods": "POST, OPTIONS",
	"Access-Control-Allow-Headers": "Content-Type",
};

function json(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { ...CORS_HEADERS, "Content-Type": "application/json" },
	});
}

async function callGroq(env: Env, systemPrompt: string, messages: { role: string; content: string }[]) {
	return fetch("https://api.groq.com/openai/v1/chat/completions", {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			Authorization: `Bearer ${env.GROQ_API_KEY}`,
		},
		body: JSON.stringify({
			model: "llama-3.3-70b-versatile",
			messages: [{ role: "system", content: systemPrompt }, ...messages],
			response_format: { type: "json_object" },
			temperature: 0.4,
		}),
	});
}

const FALLBACK_CLARIFY = {
	type: "clarify",
	question: "Could you tell me a bit more about what you're in the mood for?",
	options: null,
	multiSelect: false,
};

const UNREACHABLE_CLARIFY = {
	type: "clarify",
	question: "I'm having trouble reaching the recommendation service right now — try again in a moment.",
	options: null,
	multiSelect: false,
};

async function handleChat(request: Request, env: Env): Promise<Response> {
	let body: ChatRequestBody;
	try {
		body = await request.json();
	} catch {
		return json(
			{ type: "clarify", question: "Sorry, I didn't catch that — could you rephrase?", options: null, multiSelect: false },
			400,
		);
	}

	const messages = [
		...body.history.map((m) => ({ role: m.role, content: m.content })),
		{ role: "user", content: body.message },
	];

	if (!body.awaitingAnswer) {
		const groqResponse = await callGroq(env, CLARIFY_SYSTEM_PROMPT, messages);
		if (!groqResponse.ok) return json(UNREACHABLE_CLARIFY, 502);

		const groqData = await groqResponse.json<{ choices: { message: { content: string } }[] }>();
		const raw = groqData.choices?.[0]?.message?.content ?? "";

		try {
			const parsed = JSON.parse(raw) as { question: string; options?: string[] | null; multiSelect?: boolean };
			return json({
				type: "clarify",
				question: parsed.question,
				options: parsed.options ?? null,
				multiSelect: !!parsed.multiSelect,
			});
		} catch {
			return json(FALLBACK_CLARIFY);
		}
	}

	const groqResponse = await callGroq(env, SEARCH_SYSTEM_PROMPT, messages);
	if (!groqResponse.ok) return json(UNREACHABLE_CLARIFY, 502);

	const groqData = await groqResponse.json<{ choices: { message: { content: string } }[] }>();
	const raw = groqData.choices?.[0]?.message?.content ?? "";

	try {
		const parsed = JSON.parse(raw) as { titles?: string[]; reasoning?: string };
		return json({ type: "search", titles: parsed.titles ?? [], reasoning: parsed.reasoning ?? "" });
	} catch {
		return json(FALLBACK_CLARIFY);
	}
}

async function handleSuggest(request: Request, env: Env): Promise<Response> {
	let body: SuggestRequestBody;
	try {
		body = await request.json();
	} catch {
		return json({ titles: [], reasoning: "" }, 400);
	}

	const gamesList = body.games.map((g) => `${g.name}${g.genre ? ` (${g.genre})` : ""}`).join(", ");
	const messages = [{ role: "user", content: `Player's favorite games: ${gamesList}` }];

	const groqResponse = await callGroq(env, SUGGEST_SYSTEM_PROMPT, messages);

	if (!groqResponse.ok) {
		return json({ titles: [], reasoning: "" }, 502);
	}

	const groqData = await groqResponse.json<{ choices: { message: { content: string } }[] }>();
	const raw = groqData.choices?.[0]?.message?.content ?? "";

	try {
		return json(JSON.parse(raw));
	} catch {
		return json({ titles: [], reasoning: "" });
	}
}

export default {
	async fetch(request, env, _ctx): Promise<Response> {
		if (request.method === "OPTIONS") {
			return new Response(null, { headers: CORS_HEADERS });
		}

		if (request.method !== "POST") {
			return json({ error: "not found" }, 404);
		}

		const url = new URL(request.url);
		if (url.pathname === "/chat") {
			return handleChat(request, env);
		}
		if (url.pathname === "/suggest") {
			return handleSuggest(request, env);
		}
		return json({ error: "not found" }, 404);
	},
} satisfies ExportedHandler<Env>;
