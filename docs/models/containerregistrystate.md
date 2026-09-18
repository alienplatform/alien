# ContainerRegistryState

## Example Usage

```typescript
import { ContainerRegistryState } from "@alienplatform/platform-api/models";

let value: ContainerRegistryState = {
  route: null,
  endpoint: null,
  repositories: [],
  credentials: [
    {
      id: "crcred_oz1xjr82f37j17g4gtmyu",
      label: "<value>",
      scope: "pushPull",
      repositorySubset: [
        "<value 1>",
      ],
      expiresAt: new Date("2024-10-29T23:56:01.394Z"),
      lastUsedAt: new Date("2026-09-08T03:35:29.050Z"),
      revokedAt: new Date("2024-10-03T21:49:18.137Z"),
      createdAt: new Date("2024-05-23T04:16:56.268Z"),
    },
  ],
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `route`                                                                                    | [models.ContainerRegistryStateRoute](../models/containerregistrystateroute.md)             | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `endpoint`                                                                                 | [models.Endpoint](../models/endpoint.md)                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `repositories`                                                                             | [models.ContainerRegistryStateRepository](../models/containerregistrystaterepository.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `credentials`                                                                              | [models.ContainerRegistryStateCredential](../models/containerregistrystatecredential.md)[] | :heavy_check_mark:                                                                         | N/A                                                                                        |