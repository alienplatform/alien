# ResolveManagerGcpOAuthProviderRequest

## Example Usage

```typescript
import { ResolveManagerGcpOAuthProviderRequest } from "@alienplatform/platform-api/models";

let value: ResolveManagerGcpOAuthProviderRequest = {
  deploymentGroupToken: "<value>",
};
```

## Fields

| Field                                                                                                                                                                     | Type                                                                                                                                                                      | Required                                                                                                                                                                  | Description                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deploymentGroupToken`                                                                                                                                                    | *string*                                                                                                                                                                  | :heavy_check_mark:                                                                                                                                                        | Deployment-group bearer token whose project-level OAuth provider should be resolved.                                                                                      |
| `returnOrigin`                                                                                                                                                            | *string*                                                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                        | Browser origin that will receive the Google OAuth callback result. Must be a first-party dashboard origin or the active portal origin for the deployment group's project. |