# CreateDeploymentTokenResponse

## Example Usage

```typescript
import { CreateDeploymentTokenResponse } from "@alienplatform/platform-api/models";

let value: CreateDeploymentTokenResponse = {
  token: "<value>",
  deploymentId: "<id>",
  expiresAt: new Date("2025-11-11T09:49:38.810Z"),
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `token`                                                                                                                                              | *string*                                                                                                                                             | :heavy_check_mark:                                                                                                                                   | The generated deployment token (only shown once)                                                                                                     |
| `deploymentId`                                                                                                                                       | *string*                                                                                                                                             | :heavy_check_mark:                                                                                                                                   | The deployment ID that this token is scoped to                                                                                                       |
| `expiresAt`                                                                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                        | :heavy_check_mark:                                                                                                                                   | When the token expires, or null if never. A key below the workspace cannot mint a token that outlives itself, so this can be earlier than requested. |