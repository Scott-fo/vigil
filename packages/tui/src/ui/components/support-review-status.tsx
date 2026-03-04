import { memo } from "react";
import "opentui-spinner/react";
import type { ResolvedTheme } from "#theme/theme.ts";

interface SupportReviewStatusProps {
	readonly theme: ResolvedTheme;
	readonly loading: boolean;
	readonly top?: number;
}

export const SupportReviewStatus = memo(function SupportReviewStatus(
	props: SupportReviewStatusProps,
) {
	if (!props.loading) {
		return null;
	}

	return (
		<box
			position="absolute"
			top={props.top ?? 1}
			right={1}
			zIndex={122}
			paddingX={1}
			paddingY={0}
			border
			borderStyle="rounded"
			borderColor={props.theme.primary}
			backgroundColor={props.theme.backgroundPanel}
			flexDirection="row"
			alignItems="center"
			gap={1}
		>
			<spinner name="dots" color={props.theme.primary} />
			<text fg={props.theme.textMuted}>Generating review...</text>
		</box>
	);
});
