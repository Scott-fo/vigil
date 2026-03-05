import { describe, expect, test } from "bun:test";
import { resolveManagedDaemonCommand } from "./supervisor.ts";

describe("resolveManagedDaemonCommand", () => {
	test("prefers the explicit self executable from the install wrapper", () => {
		const launch = resolveManagedDaemonCommand(
			{
				execPath: "/home/scottfo/.local/lib/vigil/vigil",
				argv: ["/home/scottfo/.local/lib/vigil/vigil"],
				bunMain: "/$bunfs/root/src/index.tsx",
				selfExecutable: "/home/scottfo/.local/lib/vigil/vigil",
			},
			() => false,
		);

		expect(launch).toEqual({
			command: "/home/scottfo/.local/lib/vigil/vigil",
			args: [],
		});
	});

	test("uses the bun entrypoint when running as a script", () => {
		const launch = resolveManagedDaemonCommand(
			{
				execPath: "/home/scottfo/.bun/bin/bun",
				argv: ["/home/scottfo/.bun/bin/bun", "/repo/bin/vigil"],
				bunMain: "/repo/bin/vigil",
				selfExecutable: undefined,
			},
			(path) => path === "/repo/bin/vigil",
		);

		expect(launch).toEqual({
			command: "/home/scottfo/.bun/bin/bun",
			args: ["/repo/bin/vigil"],
		});
	});

	test("falls back to argv[1] when bunMain is unavailable", () => {
		const launch = resolveManagedDaemonCommand(
			{
				execPath: "/home/scottfo/.bun/bin/bun",
				argv: ["/home/scottfo/.bun/bin/bun", "/repo/bin/vigil"],
				bunMain: undefined,
				selfExecutable: undefined,
			},
			(path) => path === "/repo/bin/vigil",
		);

		expect(launch).toEqual({
			command: "/home/scottfo/.bun/bin/bun",
			args: ["/repo/bin/vigil"],
		});
	});

	test("spawns the current executable directly when there is no script entrypoint", () => {
		const launch = resolveManagedDaemonCommand(
			{
				execPath: "/tmp/vigil-install/vigil",
				argv: ["/tmp/vigil-install/vigil", "serve"],
				bunMain: "/$bunfs/root/src/index.tsx",
				selfExecutable: undefined,
			},
			() => false,
		);

		expect(launch).toEqual({
			command: "/tmp/vigil-install/vigil",
			args: [],
		});
	});
});
