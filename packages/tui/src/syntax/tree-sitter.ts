import {
	addDefaultParsers,
	getTreeSitterClient,
	pathToFiletype,
	type FiletypeParserOptions,
	type TreeSitterClient,
} from "@opentui/core";
import { Data, Effect, Match, Option, pipe, Schema } from "effect";
import parserConfig from "#config/parsers";

const ParserConfigSchema = Schema.Struct({
	parsers: Schema.optional(Schema.Array(Schema.Unknown)),
});

const InjectionMappingSchema = Schema.Struct({
	nodeTypes: Schema.optional(
		Schema.Record({
			key: Schema.String,
			value: Schema.String,
		}),
	),
	infoStringMap: Schema.optional(
		Schema.Record({
			key: Schema.String,
			value: Schema.String,
		}),
	),
});

const ParserQueriesSchema = Schema.Struct({
	highlights: Schema.Array(Schema.NonEmptyString).pipe(Schema.minItems(1)),
	injections: Schema.optional(Schema.Array(Schema.NonEmptyString)),
});

const ParserEntrySchema = Schema.Struct({
	filetype: Schema.NonEmptyString,
	wasm: Schema.NonEmptyString,
	queries: ParserQueriesSchema,
	injectionMapping: Schema.optional(InjectionMappingSchema),
});

type ParserEntry = Schema.Schema.Type<typeof ParserEntrySchema>;

export class TreeSitterInitializeError extends Data.TaggedError(
	"TreeSitterInitializeError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

let parserOptionsCache: Option.Option<ReadonlyArray<FiletypeParserOptions>> =
	Option.none();
let treeSitterInitPromise: Option.Option<Promise<TreeSitterClient>> =
	Option.none();

function normalizeInjectionMapping(
	mapping: ParserEntry["injectionMapping"],
): Option.Option<NonNullable<FiletypeParserOptions["injectionMapping"]>> {
	return pipe(
		Option.fromNullable(mapping),
		Option.flatMap((resolvedMapping) => {
			const normalized: NonNullable<FiletypeParserOptions["injectionMapping"]> =
				{};
			pipe(
				Option.fromNullable(resolvedMapping.nodeTypes),
				Option.match({
					onNone: () => {},
					onSome: (nodeTypes) => {
						normalized.nodeTypes = { ...nodeTypes };
					},
				}),
			);
			pipe(
				Option.fromNullable(resolvedMapping.infoStringMap),
				Option.match({
					onNone: () => {},
					onSome: (infoStringMap) => {
						normalized.infoStringMap = { ...infoStringMap };
					},
				}),
			);
			return Match.value(Object.keys(normalized).length).pipe(
				Match.when(0, () => Option.none()),
				Match.orElse(() => Option.some(normalized)),
			);
		}),
	);
}

function toFiletypeParserOptions(parser: ParserEntry): FiletypeParserOptions {
	const queries: FiletypeParserOptions["queries"] = {
		highlights: [...parser.queries.highlights],
	};
	pipe(
		Option.fromNullable(parser.queries.injections),
		Option.filter((injections) => injections.length > 0),
		Option.match({
			onNone: () => {},
			onSome: (injections) => {
				queries.injections = [...injections];
			},
		}),
	);

	const parserOptions: FiletypeParserOptions = {
		filetype: parser.filetype,
		wasm: parser.wasm,
		queries,
	};
	pipe(
		normalizeInjectionMapping(parser.injectionMapping),
		Option.match({
			onNone: () => {},
			onSome: (injectionMapping) => {
				parserOptions.injectionMapping = injectionMapping;
			},
		}),
	);
	return parserOptions;
}

function getParserOptions(): FiletypeParserOptions[] {
	return pipe(
		parserOptionsCache,
		Option.match({
			onSome: (cached) => [...cached],
			onNone: () => {
				const config = Effect.runSync(
					pipe(
						Schema.decodeUnknown(ParserConfigSchema)(parserConfig),
						Effect.match({
							onFailure: () =>
								({
									parsers: [],
								}) as Schema.Schema.Type<typeof ParserConfigSchema>,
							onSuccess: (decoded) => decoded,
						}),
					),
				);
				const list = pipe(
					Option.fromNullable(config.parsers),
					Option.getOrElse(() => [] as Array<unknown>),
				);

				const parserOptions = list.flatMap((entry) => {
					const parsedOption = Effect.runSync(
						pipe(
							Schema.decodeUnknown(ParserEntrySchema)(entry),
							Effect.match({
								onFailure: () => Option.none<FiletypeParserOptions>(),
								onSuccess: (decoded) =>
									Option.some(toFiletypeParserOptions(decoded)),
							}),
						),
					);
					return pipe(
						parsedOption,
						Option.match({
							onNone: () => [] as Array<FiletypeParserOptions>,
							onSome: (resolved) => [resolved],
						}),
					);
				});

				parserOptionsCache = Option.some(parserOptions);
				return parserOptions;
			},
		}),
	);
}

export function initializeTreeSitterClient(): Effect.Effect<
	TreeSitterClient,
	TreeSitterInitializeError
> {
	return Effect.tryPromise({
		try: () =>
			pipe(
				treeSitterInitPromise,
				Option.match({
					onSome: (initializingClient) => initializingClient,
					onNone: () => {
						addDefaultParsers(getParserOptions());
						const client = getTreeSitterClient();
						const initializingClient = client.initialize().then(
							() => client,
							(cause) => {
								treeSitterInitPromise = Option.none();
								throw cause;
							},
						);
						treeSitterInitPromise = Option.some(initializingClient);
						return initializingClient;
					},
				}),
			),
		catch: (cause) =>
			new TreeSitterInitializeError({
				message: "Failed to initialize Tree-sitter client.",
				cause,
			}),
	});
}

function extractFileName(filePath: string): string {
	const loweredPath = filePath.toLowerCase();
	const segments = loweredPath.split("/");
	return pipe(
		Option.fromNullable(segments[segments.length - 1]),
		Option.getOrElse(() => ""),
	);
}

function extractFileExtension(fileName: string): Option.Option<string> {
	const segments = fileName.split(".");
	if (segments.length <= 1) {
		return Option.none();
	}
	return pipe(
		Option.fromNullable(segments[segments.length - 1]),
		Option.filter((extension) => extension.length > 0),
	);
}

function resolveBuiltinFiletype(
	openTuiFiletype: string,
): Option.Option<string> {
	return Match.value(openTuiFiletype).pipe(
		Match.when("typescriptreact", () => Option.some("typescript")),
		Match.when("javascriptreact", () => Option.some("typescript")),
		Match.when("javascript", () => Option.some("typescript")),
		Match.when("shell", () => Option.some("bash")),
		Match.orElse((resolved) => Option.some(resolved)),
	);
}

function resolvePatchExtensionFiletype(
	extension: string,
): Option.Option<string> {
	return Match.value(extension).pipe(
		Match.when("diff", () => Option.some("diff")),
		Match.when("patch", () => Option.some("diff")),
		Match.orElse(() => Option.none()),
	);
}

export function resolveDiffFiletype(filePath: string): Option.Option<string> {
	const fileName = extractFileName(filePath);
	return Match.value(fileName).pipe(
		Match.when("dockerfile", () => Option.some("dockerfile")),
		Match.orElse(() =>
			pipe(
				Option.fromNullable(pathToFiletype(filePath)),
				Option.match({
					onSome: (openTuiFiletype) => resolveBuiltinFiletype(openTuiFiletype),
					onNone: () =>
						pipe(
							extractFileExtension(fileName),
							Option.flatMap((extension) =>
								resolvePatchExtensionFiletype(extension),
							),
						),
				}),
			),
		),
	);
}
