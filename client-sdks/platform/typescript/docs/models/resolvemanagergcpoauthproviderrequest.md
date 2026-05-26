# ResolveManagerGcpOAuthProviderRequest

## Example Usage

```typescript
import { ResolveManagerGcpOAuthProviderRequest } from "@alienplatform/platform-api/models";

let value: ResolveManagerGcpOAuthProviderRequest = {
  deploymentGroupToken: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `deploymentGroupToken`                                                               | *string*                                                                             | :heavy_check_mark:                                                                   | Deployment-group bearer token whose project-level OAuth provider should be resolved. |