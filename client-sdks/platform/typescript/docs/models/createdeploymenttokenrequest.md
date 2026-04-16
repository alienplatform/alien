# CreateDeploymentTokenRequest

## Example Usage

```typescript
import { CreateDeploymentTokenRequest } from "@alienplatform/platform-api/models";

let value: CreateDeploymentTokenRequest = {
  description: "excluding but yum consequently",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `description`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Optional description for the agent token                                                      |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional expiration date for the agent token                                                  |