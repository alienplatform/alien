# MintCredentialsRequest

Request body for `POST /v1/credentials/mint`.

`deny_unknown_fields` so clients cannot smuggle in resolver internals
(platform, stack state, etc.) — the server derives everything from the
authenticated deployment.

## Example Usage

```typescript
import { MintCredentialsRequest } from "@alienplatform/manager-api/models";

let value: MintCredentialsRequest = {
  bindingName: "<value>",
  deploymentId: "<id>",
  resourceId: "<id>",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `bindingName`                                                                                                                                  | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Service-account binding to impersonate on the target platform.                                                                                 |
| `deploymentId`                                                                                                                                 | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Deployment to mint credentials for. The caller's bearer token must be<br/>this deployment's token (or a workspace-admin token).                |
| `durationSeconds`                                                                                                                              | *number*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Requested lifetime in seconds. Clamped to<br/>`[MIN_DURATION_SECONDS, MAX_DURATION_SECONDS]`; defaults to<br/>`DEFAULT_DURATION_SECONDS` when omitted. |
| `resourceId`                                                                                                                                   | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Current-release compute resource requesting the credentials. The<br/>resource must depend on `bindingName` as a service-account resource.      |