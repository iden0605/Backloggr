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
	// How many clarifying questions have already been asked for the CURRENT ask (resets to 0 on
	// a fresh ask, increments each time another clarifying question is shown). Once the cap is
	// hit, `handleChat` returns results from whatever candidates came back instead of asking
	// again — the conversation can never clarify forever.
	questionsAsked: number;
	// Names of games already in the player's library (any status). Recommending a game someone
	// already owns/played instantly reads as fake — the prompt forbids them and `handleChat`
	// filters any that slip through anyway.
	excluded: string[];
}

interface FavoriteGame {
	name: string;
	genre: string | null;
}

interface SuggestRequestBody {
	games: FavoriteGame[];
	// Full library (any status) — suggestions must be games the player does NOT already have.
	excluded: string[];
}

// Applies to every prompt below: the conversation history can span multiple unrelated asks in one
// session (history is never truncated, by design — so the player can keep chatting to refine or
// go deeper). When the player's latest message changes topic from what came before, treat only
// the current ask as live — earlier unrelated turns are background, not something to keep
// blending into the answer.
const TOPIC_FOCUS_NOTE = `The conversation history may contain earlier, unrelated asks from the same session (history is never cleared so the player can keep chatting). Always resolve the CURRENT ask from the most recent messages — if the player has clearly moved on to a new topic, do not keep mixing in requirements from an earlier unrelated ask.`;

// Every chat turn uses this single prompt. The model's job each round is to maintain a REAL
// candidate pool (actual titles it can name, not a fabricated count) plus the one question that
// would best split that pool. Whether to show results or ask the question is decided in
// `handleChat` from the pool's actual size — never by the model's own judgment, which we've
// learned not to trust for flow control (it used to skip clarifying entirely when asked to
// self-judge). A specific first message can therefore get instant results (small pool), while a
// vague one naturally enters a narrowing loop (big pool → question → smaller pool → ...).
function narrowSystemPrompt(questionsAsked: number, excluded: string[]): string {
	const exclusionNote =
		excluded.length > 0
			? `\nThe player already has these games in their library — NEVER include any of them (or a remaster/edition of one) as a candidate: ${excluded.join(", ")}.\n`
			: "";
	return `A player is asking for game recommendations (see the conversation so far — they've answered ${questionsAsked} clarifying question(s) for the current ask).

${TOPIC_FOCUS_NOTE}
${exclusionNote}
Your job each turn, in two parts:

1. CANDIDATES — list the real, specific games you know of that fit everything the player has said so far in the current ask. The list's size must honestly reflect how narrowed-down the request is:
   - Vague or broad request → 12 to 20 genuinely diverse candidates spanning the plausible interpretations.
   - Well-specified request → only the games that truly fit, even if that's just 4-6.
   Every candidate needs a "reason": a short phrase (under 12 words) tying it to what THIS player asked for — not a generic blurb. Never pad the list with reskins/sequels/near-duplicates of the same game, and prioritize variety across developers/series.
   If the player signals they just want results now ("just show me", "surprise me", "whatever you think"), cut the list to your best 8 or fewer regardless of how broad the ask still is.

2. QUESTION — if your candidate list has more than 8 entries, also write the ONE question whose answer would best split the list into meaningfully different subsets (setting, tone, pacing, difficulty, social angle, art style, a defining mechanic, what they loved about a game they named...). Each option you offer should correspond to a real subset of your candidates. Never re-ask something the player already answered, and don't repeat an axis you already asked about in this ask. If your list is already 8 or fewer, set "question" to null.

Also write "reasoning": one short sentence summarizing why this set fits the ask (used as the lead-in when results are shown).

Reply with ONLY strict JSON, no prose, no markdown fences, in this exact shape:
{"candidates": [{"title": "<specific real game title>", "reason": "<why it fits, under 12 words>"}, ...], "reasoning": "<one short sentence>", "question": "<one short question>" or null, "options": ["<short choice>", "..."] or null, "multiSelect": <true or false>}

Set "multiSelect": true when more than one option could reasonably apply at once; false for an either/or choice. Use "options": null only for a genuinely open-ended question.`;
}

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
	question: "Could you tell me a bit more about what you're in the mood for?",
	options: null,
	multiSelect: false,
	candidateCount: null,
};

const UNREACHABLE_CLARIFY = {
	question: "I'm having trouble reaching the recommendation service right now — try again in a moment.",
	options: null,
	multiSelect: false,
	candidateCount: null,
};

async function handleChat(request: Request, env: Env): Promise<Response> {
	let body: ChatRequestBody;
	try {
		body = await request.json();
	} catch {
		return json(
			{ question: "Sorry, I didn't catch that — could you rephrase?", options: null, multiSelect: false, candidateCount: null },
			400,
		);
	}

	const messages = [
		...body.history.map((m) => ({ role: m.role, content: m.content })),
		{ role: "user", content: body.message },
	];

	const questionsAsked = body.questionsAsked ?? 0;
	const excluded = body.excluded ?? [];

	const groqResponse = await callGroq(env, narrowSystemPrompt(questionsAsked, excluded), messages);
	if (!groqResponse.ok) return json(UNREACHABLE_CLARIFY, 502);

	const groqData = await groqResponse.json<{ choices: { message: { content: string } }[] }>();
	const raw = groqData.choices?.[0]?.message?.content ?? "";

	let parsed: {
		candidates?: { title?: string; reason?: string }[];
		reasoning?: string;
		question?: string | null;
		options?: string[] | null;
		multiSelect?: boolean;
	};
	try {
		parsed = JSON.parse(raw);
	} catch {
		return json(FALLBACK_CLARIFY);
	}

	// Belt-and-braces on top of the prompt: drop any owned title that slipped through anyway.
	const excludedLower = new Set(excluded.map((n) => n.toLowerCase()));
	const candidates = (parsed.candidates ?? [])
		.filter((c): c is { title: string; reason?: string } => typeof c?.title === "string" && c.title.length > 0)
		.filter((c) => !excludedLower.has(c.title.toLowerCase()))
		.map((c) => ({ title: c.title, reason: c.reason ?? null }));

	// The show-results-vs-keep-narrowing decision lives HERE, not in the model: a focused pool
	// (or a hit round cap, or the model returning no question) means results now; otherwise ask
	// the pool-splitting question and report the real pool size so the UI can show honest
	// narrowing progress.
	const capReached = questionsAsked >= 4;
	if (candidates.length > 0 && (candidates.length <= 8 || capReached || !parsed.question)) {
		return json({ titles: candidates.slice(0, 8), reasoning: parsed.reasoning ?? "" });
	}

	if (parsed.question) {
		return json({
			question: parsed.question,
			options: parsed.options ?? null,
			multiSelect: !!parsed.multiSelect,
			candidateCount: candidates.length > 0 ? candidates.length : null,
		});
	}

	return json(FALLBACK_CLARIFY);
}

async function handleSuggest(request: Request, env: Env): Promise<Response> {
	let body: SuggestRequestBody;
	try {
		body = await request.json();
	} catch {
		return json({ titles: [], reasoning: "" }, 400);
	}

	const gamesList = body.games.map((g) => `${g.name}${g.genre ? ` (${g.genre})` : ""}`).join(", ");
	const excluded = body.excluded ?? [];
	const exclusionNote =
		excluded.length > 0
			? ` The player already has these games — never suggest any of them (or a remaster/edition of one): ${excluded.join(", ")}.`
			: "";
	const messages = [{ role: "user", content: `Player's favorite games: ${gamesList}.${exclusionNote}` }];

	const groqResponse = await callGroq(env, SUGGEST_SYSTEM_PROMPT, messages);

	if (!groqResponse.ok) {
		return json({ titles: [], reasoning: "" }, 502);
	}

	const groqData = await groqResponse.json<{ choices: { message: { content: string } }[] }>();
	const raw = groqData.choices?.[0]?.message?.content ?? "";

	try {
		const parsed = JSON.parse(raw) as { titles?: string[]; reasoning?: string };
		const excludedLower = new Set(excluded.map((n) => n.toLowerCase()));
		const titles = (parsed.titles ?? []).filter((t) => !excludedLower.has(t.toLowerCase()));
		return json({ titles, reasoning: parsed.reasoning ?? "" });
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
