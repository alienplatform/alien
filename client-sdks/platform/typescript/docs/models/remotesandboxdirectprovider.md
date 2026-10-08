# RemoteSandboxDirectProvider

## Example Usage

```typescript
import { RemoteSandboxDirectProvider } from "@alienplatform/platform-api/models";

let value: RemoteSandboxDirectProvider = {
  provider: "databricks",
  providerEndpoint: "<value>",
  keyFingerprint: "<value>",
  credentialStatus: "valid",
  credentialCheckedAt: new Date("2024-06-19T09:18:36.296Z"),
  catalogObservedAt: new Date("2024-07-20T20:16:10.437Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `provider`                                                                                    | [models.RemoteSandboxProvider](../models/remotesandboxprovider.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerEndpoint`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `keyFingerprint`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialStatus`                                                                            | [models.RemoteSandboxCredentialStatus](../models/remotesandboxcredentialstatus.md)            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `credentialCheckedAt`                                                                         | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `catalogObservedAt`                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |