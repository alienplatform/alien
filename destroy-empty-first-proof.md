# Empty first-deployment destroy replay

A real AWS first deployment was rejected before setup: requested 48 On-Demand vCPUs, applied quota 16, usage 0. The recorded resource inventory was empty and no management identity existed.

Before the fix, `destroy` exited 1 with the setup teardown lock error; saved status was `delete-failed`, with `CONFIGURATION_ERROR: Runtime access requires an installed management identity`.

After rebuilding the manager with the fix, the same CLI retried the same deployment and exited 0. The manager recorded `Runtime never started; skipping runtime cleanup`, with next status `Deleted`; the deployment API then returned 404.

A fresh rejected first deployment was also destroyed on its first attempt with exit 0. AWS describes after rejection and after cleanup found zero owned policies, roles, instance profiles, auto scaling groups, launch templates, security groups and nonterminated instances for both unique test prefixes. No provider writes were needed for this proof.

The standalone fix regression covers AWS/GCP/Azure and preserves normal cleanup for previous installs, prepared imports, missing inventories and recorded setup/access metadata. The live replay covers AWS only, using the integrated feature manager; it does not claim general update compatibility.
