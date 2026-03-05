import { describe, expect, test } from "bun:test";
import { createDaemonConnectionStatusReporter } from "#ui/hooks/use-daemon-session.ts";

describe("createDaemonConnectionStatusReporter", () => {
	test("dedupes repeated disconnect notices and only reconnects after a disconnect", () => {
		const events: Array<string> = [];
		const reporter = createDaemonConnectionStatusReporter({
			onDisconnect: (message) => {
				events.push(`disconnect:${message}`);
			},
			onReconnect: () => {
				events.push("reconnect");
			},
		});

		reporter.reconnect();
		reporter.disconnect("Disconnected from background daemon. Retrying...");
		reporter.disconnect("Disconnected from background daemon. Retrying...");
		reporter.reconnect();
		reporter.reconnect();

		expect(events).toEqual([
			"disconnect:Disconnected from background daemon. Retrying...",
			"reconnect",
		]);
	});

	test("emits a new disconnect notice when the failure message changes", () => {
		const events: Array<string> = [];
		const reporter = createDaemonConnectionStatusReporter({
			onDisconnect: (message) => {
				events.push(message);
			},
			onReconnect: undefined,
		});

		reporter.disconnect("Disconnected from background daemon. Retrying...");
		reporter.disconnect("Daemon token mismatch.");

		expect(events).toEqual([
			"Disconnected from background daemon. Retrying...",
			"Daemon token mismatch.",
		]);
	});
});
