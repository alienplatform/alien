# ContainerRegistryStateCredential

## Example Usage

```typescript
import { ContainerRegistryStateCredential } from "@alienplatform/platform-api/models";

let value: ContainerRegistryStateCredential = {
  id: "crcred_oz1xjr82f37j17g4gtmyu",
  label: "<value>",
  scope: "pushPull",
  repositorySubset: [
    "<value 1>",
  ],
  expiresAt: new Date("2025-07-14T04:55:52.152Z"),
  lastUsedAt: new Date("2024-07-31T14:40:33.562Z"),
  revokedAt: new Date("2026-12-23T05:59:54.307Z"),
  createdAt: new Date("2024-10-19T14:39:14.123Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the container registry credential.                                      | crcred_oz1xjr82f37j17g4gtmyu                                                                  |
| `label`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `scope`                                                                                       | [models.ContainerRegistryStateScope](../models/containerregistrystatescope.md)                | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `repositorySubset`                                                                            | *string*[]                                                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastUsedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `revokedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |