"""Drive an interactive command through the installed wheel against the real Local route."""

import asyncio

from alienplatform import AlienError, Sandbox


async def main():
    box = Sandbox("interactive-test")
    assert "interactiveCommands" in await box.capabilities()
    created = await box.create()
    try:
        command = await box.run_interactive(
            created.sandbox_id,
            "/bin/sh",
            [
                "-c",
                "trap 'echo bye; exit 130' INT; printf '> '; while read l; do echo \"=$l\"; printf '> '; done",
            ],
            timeout_ms=60_000,
        )
        stdout = ""

        async def until(expected):
            nonlocal stdout
            while not stdout.endswith(expected):
                frame = await command.__anext__()
                if frame.kind == "stdout":
                    stdout += frame.data.decode()

        await until("> ")
        await command.write(b"python\n")
        await until("=python\n> ")
        await command.signal("interrupt")
        exit_code = None
        async for frame in command:
            if frame.kind == "stdout":
                stdout += frame.data.decode()
            if frame.kind == "exit":
                exit_code = frame.exit_code
        assert exit_code == 130, exit_code
        assert stdout.endswith("bye\n"), stdout
        try:
            await command.signal("kill")
        except AlienError as error:
            assert error.code == "SANDBOX_COMMAND_FAILED"
        else:
            raise AssertionError("a signal after exit must be refused")
    finally:
        await box.terminate(created.sandbox_id)


asyncio.run(main())
