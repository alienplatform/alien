# CreateDeploymentGroupTokenResponse

## Example Usage

```typescript
import { CreateDeploymentGroupTokenResponse } from "@alienplatform/platform-api/models";

let value: CreateDeploymentGroupTokenResponse = {
  token: "<value>",
  deploymentLink: "<value>",
  expiresAt: new Date("2025-06-09T09:25:55.514Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `token`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | The API key token                                                                             |
| `deploymentLink`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | Formatted deployment link                                                                     |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the setup link and its API key expire                                                    |