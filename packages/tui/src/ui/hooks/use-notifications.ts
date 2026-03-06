import { useRenderer } from "@opentui/react";
import { Effect, Match, Option, pipe } from "effect";
import { useCallback, useEffect, useRef, useState } from "react";
import { copyTextToClipboard } from "#data/clipboard.ts";
import type { SnackbarNotice } from "#ui/components/snackbar.tsx";

interface UseNotificationsOptions {
	readonly renderer: ReturnType<typeof useRenderer>;
	readonly hasRemoteSyncRunning: boolean;
}

export function useNotifications(options: UseNotificationsOptions) {
	const { renderer, hasRemoteSyncRunning } = options;
	const [daemonSnackbarNotice, setDaemonSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const [transientSnackbarNotice, setTransientSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const snackbarTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	const showSnackbar = useCallback((notice: SnackbarNotice) => {
		if (snackbarTimeoutRef.current) {
			clearTimeout(snackbarTimeoutRef.current);
		}

		setTransientSnackbarNotice(Option.some(notice));

		const timeoutHandle = setTimeout(() => {
			setTransientSnackbarNotice(Option.none());
		}, 2000);

		timeoutHandle.unref?.();
		snackbarTimeoutRef.current = timeoutHandle;
	}, []);

	const notifyDaemonDisconnected = useCallback((message: string) => {
		setDaemonSnackbarNotice(
			Option.some({
				message,
				variant: "error",
			}),
		);
	}, []);

	const notifyDaemonReconnected = useCallback(() => {
		setDaemonSnackbarNotice(Option.none());
		showSnackbar({
			message: "Reconnected to background daemon",
			variant: "info",
		});
	}, [showSnackbar]);

	const copySelection = useCallback(
		(text: string) => {
			if (text.length === 0) {
				return;
			}

			void Effect.runPromise(
				pipe(
					copyTextToClipboard(renderer, text),
					Effect.match({
						onFailure: (error) => {
							showSnackbar({
								message: Match.value(error).pipe(
									Match.tag(
										"NativeClipboardCopyError",
										(typedError) => typedError.message,
									),
									Match.tag(
										"ClipboardUnavailableError",
										(typedError) => typedError.message,
									),
									Match.exhaustive,
								),
								variant: "error",
							});
						},
						onSuccess: () => {
							showSnackbar({
								message: "Text copied to clipboard",
								variant: "info",
							});
						},
					}),
				),
			);
		},
		[renderer, showSnackbar],
	);

	const onCopySelection = useCallback(() => {
		const text = renderer.getSelection()?.getSelectedText();
		if (!text) {
			return;
		}

		copySelection(text);
		renderer.clearSelection();
	}, [copySelection, renderer]);

	useEffect(
		() => () => {
			if (snackbarTimeoutRef.current) {
				clearTimeout(snackbarTimeoutRef.current);
			}
		},
		[],
	);

	useEffect(() => {
		renderer.console.onCopySelection = (text: string) => {
			if (!text) {
				return;
			}

			copySelection(text);
			renderer.clearSelection();
		};

		return () => {
			renderer.console.onCopySelection = undefined;
		};
	}, [copySelection, renderer]);

	const snackbarTop = hasRemoteSyncRunning ? 4 : 1;
	const transientSnackbarTop = Option.isSome(daemonSnackbarNotice)
		? snackbarTop + 4
		: snackbarTop;

	return {
		daemonSnackbarNotice,
		notifyDaemonDisconnected,
		notifyDaemonReconnected,
		onCopySelection,
		snackbarTop,
		transientSnackbarNotice,
		transientSnackbarTop,
	};
}
