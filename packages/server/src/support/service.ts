import { createOpencode, type Part } from "@opencode-ai/sdk";
import { Context, Data, Effect, Layer, Option } from "effect";

const OPENCODE_SERVER_PORT = 4097;
type OpencodeInstance = Awaited<ReturnType<typeof createOpencode>>;

interface ReviewDiffInput {
	readonly repoRoot: string;
	readonly mode: "working-tree" | "branch-compare";
	readonly sourceRef: Option.Option<string>;
	readonly destinationRef: Option.Option<string>;
}

export class SupportServiceValidationError extends Data.TaggedError(
	"SupportServiceValidationError",
)<{
	readonly message: string;
}> {}

export class SupportServiceOpencodeError extends Data.TaggedError(
	"SupportServiceOpencodeError",
)<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

export type SupportServiceError =
	| SupportServiceValidationError
	| SupportServiceOpencodeError;

function toNullableString(value: Option.Option<string>) {
	return Option.match(value, {
		onNone: () => null,
		onSome: (value) => value,
	});
}

function trimAndValidate(
	field: string,
	value: string,
): Effect.Effect<string, SupportServiceValidationError> {
	const normalized = value.trim();
	return normalized.length > 0
		? Effect.succeed(normalized)
		: Effect.fail(
				new SupportServiceValidationError({
					message: `${field} must not be empty.`,
				}),
			);
}

function formatPrompt(input: {
	readonly mode: "working-tree" | "branch-compare";
	readonly sourceRef: string | null;
	readonly destinationRef: string | null;
}) {
	if (
		input.mode === "branch-compare" &&
		input.sourceRef !== null &&
		input.destinationRef !== null
	) {
		return [
			"Please review the diff between these two references",
			`Context: compare ${input.destinationRef}...${input.sourceRef}.`,
		].join("\n");
	}

	return "Please tell me about the history of react and give me some examples of hooks vs the old class based design";
}

function renderOpencodeError(cause: unknown) {
	if (cause instanceof Error && cause.message.length > 0) {
		return cause.message;
	}

	return "opencode request failed.";
}

function extractMarkdown(parts: ReadonlyArray<Part>): string {
	const text = parts
		.filter(
			(part): part is Extract<Part, { type: "text" }> => part.type === "text",
		)
		.map((part) => part.text.trim())
		.filter((part) => part.length > 0)
		.join("\n\n")
		.trim();

	return text.length > 0
		? text
		: "opencode returned no text content for this review.";
}

export class SupportService extends Context.Tag("@vigil/server/SupportService")<
	SupportService,
	{
		readonly reviewDiff: (
			input: ReviewDiffInput,
		) => Effect.Effect<string, SupportServiceError>;
	}
>() {
	static readonly layer = Layer.scoped(
		SupportService,
		Effect.gen(function* () {
			let opencodeInstancePromise: Promise<OpencodeInstance> | null = null;

			const getOrCreateOpencodeInstance = () => {
				if (opencodeInstancePromise) {
					return opencodeInstancePromise;
				}

				opencodeInstancePromise = createOpencode({
					port: OPENCODE_SERVER_PORT,
				}).catch((error) => {
					opencodeInstancePromise = null;
					throw error;
				});

				return opencodeInstancePromise;
			};

			yield* Effect.addFinalizer(() =>
				Effect.tryPromise({
					try: async () => {
						const active = opencodeInstancePromise;
						opencodeInstancePromise = null;
						if (!active) {
							return;
						}

						const opencode = await active.catch(() => null);
						opencode?.server.close();
					},
					catch: () => undefined,
				}).pipe(
					Effect.catchAll(() => Effect.void),
				),
			);

			const reviewDiff = Effect.fn("SupportService.reviewDiff")(function* (
				input: ReviewDiffInput,
			) {
				const opencode = yield* Effect.tryPromise({
					try: () => getOrCreateOpencodeInstance(),
					catch: (cause) =>
						new SupportServiceOpencodeError({
							message: `Failed to initialize opencode. ${renderOpencodeError(cause)}`,
							cause,
						}),
				});

				const repoRoot = yield* trimAndValidate("repoRoot", input.repoRoot);
				const sourceRef = toNullableString(input.sourceRef);
				const destinationRef = toNullableString(input.destinationRef);
				const prompt = formatPrompt({
					mode: input.mode,
					sourceRef,
					destinationRef,
				});

				const session = yield* Effect.tryPromise({
					try: () =>
						opencode.client.session.create({
							query: {
								directory: repoRoot,
							},
							body: {
								title: "Vigil Diff Review",
							},
						}),
					catch: (cause) =>
						new SupportServiceOpencodeError({
							message: `Unable to create opencode session. ${renderOpencodeError(cause)}`,
							cause,
						}),
				});

				if (session.error || !session.data) {
					return yield* new SupportServiceOpencodeError({
						message: "Unable to create opencode session.",
						cause: session.error,
					});
				}

				const response = yield* Effect.tryPromise({
					try: () =>
						opencode.client.session.prompt({
							path: {
								id: session.data.id,
							},
							query: {
								directory: repoRoot,
							},
							body: {
								parts: [
									{
										type: "text",
										text: prompt,
									},
								],
							},
						}),
					catch: (cause) =>
						new SupportServiceOpencodeError({
							message: `Unable to request review from opencode. ${renderOpencodeError(cause)}`,
							cause,
						}),
				});

				if (response.error || !response.data) {
					return yield* new SupportServiceOpencodeError({
						message: "Unable to request review from opencode.",
						cause: response.error,
					});
				}

				const assistantError = response.data.info.error;
				if (assistantError) {
					const details =
						"data" in assistantError && assistantError.data
							? JSON.stringify(assistantError.data)
							: assistantError.name;
					return yield* new SupportServiceOpencodeError({
						message: `opencode returned an assistant error (${assistantError.name}): ${details}`,
					});
				}

				return extractMarkdown(response.data.parts);
			});

			return SupportService.of({
				reviewDiff,
			});
		}),
	);
}
