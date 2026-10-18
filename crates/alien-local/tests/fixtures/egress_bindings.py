"""Exercise the installed wheel against the real Local route supplied by the Rust harness."""

import asyncio

from alienplatform import AlienError, Sandbox


async def main():
    box = Sandbox("egress-test")
    created = await box.create()
    try:
        found, was_created = await box.get_or_create(sandbox_id=created.sandbox_id)
        assert not was_created and found.sandbox_id == created.sandbox_id
        for call in (
            lambda: box.create(egress="internet"),
            lambda: box.create(egress={"allow": []}),
            lambda: box.get_or_create(
                sandbox_id=created.sandbox_id, egress={"allow": []}
            ),
            lambda: box.update_egress(created.sandbox_id, {"allow": []}),
        ):
            try:
                await call()
            except AlienError as error:
                assert (
                    error.code == "SANDBOX_EGRESS_REQUIRES_INTERNET"
                    and not error.retryable
                )
            else:
                raise AssertionError("Legacy declarations must refuse runtime policies")
    finally:
        await box.terminate(created.sandbox_id)

    try:
        await Sandbox("egress-unsupported").create()
    except AlienError as error:
        assert (
            error.code == "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE" and not error.retryable
        )
    else:
        raise AssertionError(
            "Internet without a gateway must be refused before creation"
        )


asyncio.run(main())
