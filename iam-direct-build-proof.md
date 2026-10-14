# Direct IAM role build-time proof

The original comparison parent (`aa42e2f38`) and the saved earlier feature CLI (session 51) accept two direct ServiceAccounts whose global documents each need 10,783 non-whitespace characters. Each of their two permission sets fits separately; their combined role document exceeds 10,240.

The updated CLI rejects the same fixture at `alien build` with both role shortfalls in one result, exit 1 and zero output artifacts. The small control builds successfully, exit 0, one stack artifact. These runs strip cloud credential environment variables and perform no deployment. Fixtures, exact commands, raw outputs and result manifests are adjacent.

Only definite literal global grants form this lower bound. Resource-scoped, injected, dynamic and existing role documents can consume further headroom. Direct config grants replaced by a matching permission profile during setup are excluded; the profile is checked instead.

This supplements the separately recorded real IAM direct-deployment failure and positive control. It does not establish full role aggregation across resource controllers or native CloudFormation/Terraform apply behavior.
