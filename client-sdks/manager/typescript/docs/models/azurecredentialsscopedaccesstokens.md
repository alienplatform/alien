# AzureCredentialsScopedAccessTokens

Short-lived bearer tokens keyed by their exact Azure OAuth scope.

This is the only Azure credential form returned by the credential mint
endpoint. It contains no refreshable source credential and must not be
used for a scope that is absent from the map.

## Example Usage

```typescript
import { AzureCredentialsScopedAccessTokens } from "@alienplatform/manager-api/models";

let value: AzureCredentialsScopedAccessTokens = {
  tokens: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  type: "scopedAccessTokens",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `tokens`                                                                                                                                       | Record<string, *string*>                                                                                                                       | :heavy_check_mark:                                                                                                                             | Exact scope-to-token map. Minted configs include only the Azure<br/>management, storage, Key Vault, and Service Bus scopes used by<br/>Alien bindings. |
| `type`                                                                                                                                         | *"scopedAccessTokens"*                                                                                                                         | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |