import * as PlatformError from "@effect/platform/Error";
import * as FileSystem from "@effect/platform/FileSystem";
import * as ParcelWatcher from "@parcel/watcher";
import { Chunk, Effect, Layer, Option, Stream } from "effect";

const PARCEL_WATCH_IGNORE: ReadonlyArray<string> = [
	".*/**",
	"**/.*/**",
	"**/node_modules/**",
];

function toWatchEvent(event: ParcelWatcher.Event): FileSystem.WatchEvent {
	switch (event.type) {
		case "create":
			return FileSystem.WatchEventCreate({ path: event.path });
		case "update":
			return FileSystem.WatchEventUpdate({ path: event.path });
		case "delete":
			return FileSystem.WatchEventRemove({ path: event.path });
	}
}

const watchParcel = (path: string) =>
	Stream.asyncScoped<FileSystem.WatchEvent, PlatformError.PlatformError>(
		(emit) =>
			Effect.acquireRelease(
				Effect.tryPromise({
					try: () =>
						ParcelWatcher.subscribe(
							path,
							(cause, events) => {
								if (cause) {
									emit.fail(
										new PlatformError.SystemError({
											reason: "Unknown",
											module: "FileSystem",
											method: "watch",
											pathOrDescriptor: path,
											cause,
										}),
									);
									return;
								}

								emit.chunk(Chunk.unsafeFromArray(events.map(toWatchEvent)));
							},
							{
								ignore: [...PARCEL_WATCH_IGNORE],
							},
						),
					catch: (cause) =>
						new PlatformError.SystemError({
							reason: "Unknown",
							module: "FileSystem",
							method: "watch",
							pathOrDescriptor: path,
							cause,
						}),
				}),
				(subscription) => Effect.promise(() => subscription.unsubscribe()),
			),
	);

const backend = FileSystem.WatchBackend.of({
	register(path, stat) {
		if (stat.type !== "Directory") {
			return Option.none();
		}

		return Option.some(watchParcel(path));
	},
});

export const parcelWatchBackendLayer = Layer.succeed(
	FileSystem.WatchBackend,
	backend,
);
