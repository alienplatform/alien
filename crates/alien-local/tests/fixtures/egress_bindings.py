"""Exercise the installed wheel against the real Local route supplied by the Rust harness."""
import asyncio

from alienplatform import AlienError, Sandbox


async def main():
    box = Sandbox("egress-test")
    created = await box.create(egress="internet")
    try:
        found, was_created = await box.get_or_create(sandbox_id=created.sandbox_id, egress="internet")
        assert not was_created and found.sandbox_id == created.sandbox_id
        for call in (
            lambda: box.create(egress="none"),
            lambda: box.get_or_create(sandbox_id=created.sandbox_id, egress="none"),
            lambda: box.update_egress(created.sandbox_id, "none"),
        ):
            try:
                await call()
            except AlienError as error:
                assert error.code == "OPERATION_NOT_SUPPORTED" and not error.retryable
            else:
                raise AssertionError("Local cannot yet enforce a narrowed policy")
        try:
            await box.create(egress={"allow": []})
        except AlienError as error:
            assert error.code == "SANDBOX_EGRESS_INVALID"
        else:
            raise AssertionError("empty target lists must be refused")
        try:
            await box.create(egress={"allow": ["example.com"] * 4097})
        except AlienError as error:
            assert error.code == "SANDBOX_EGRESS_POLICY_TOO_LARGE" and not error.retryable
        else:
            raise AssertionError("oversized policies must be refused")
    finally:
        await box.terminate(created.sandbox_id)


asyncio.run(main())
