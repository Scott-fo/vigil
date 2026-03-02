import { Option } from "effect";
import { memo } from "react";
import type { ResolvedTheme } from "#theme/theme";

export type SnackbarNotice = {
	readonly message: string;
	readonly variant: "info" | "error";
};

export interface SnackbarProps {
	readonly theme: ResolvedTheme;
	readonly notice: Option.Option<SnackbarNotice>;
	readonly top?: number;
}

export const Snackbar = memo(function Snackbar(props: SnackbarProps) {
	if (Option.isNone(props.notice)) {
		return null;
	}

	const borderColor =
		props.notice.value.variant === "error" ? props.theme.error : props.theme.info;

	return (
		<box
			position="absolute"
			top={props.top ?? 1}
			right={1}
			zIndex={120}
			maxWidth={56}
			paddingX={1}
			paddingY={0}
			border
			borderStyle="rounded"
			borderColor={borderColor}
			backgroundColor={props.theme.backgroundPanel}
		>
			<text fg={props.theme.text} wrapMode="word" width="100%">
				{props.notice.value.message}
			</text>
		</box>
	);
});
