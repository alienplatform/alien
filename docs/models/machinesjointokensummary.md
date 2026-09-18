# MachinesJoinTokenSummary

## Example Usage

```typescript
import { MachinesJoinTokenSummary } from "@alienplatform/platform-api/models";

let value: MachinesJoinTokenSummary = {
  id: "<id>",
  createdAt: "1717311285239",
  createdBy: "<value>",
  joinCount: 706976,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `id`               | *string*           | :heavy_check_mark: | N/A                |
| `createdAt`        | *string*           | :heavy_check_mark: | N/A                |
| `createdBy`        | *string*           | :heavy_check_mark: | N/A                |
| `expiresAt`        | *string*           | :heavy_minus_sign: | N/A                |
| `maxJoins`         | *number*           | :heavy_minus_sign: | N/A                |
| `joinCount`        | *number*           | :heavy_check_mark: | N/A                |
| `lastUsedAt`       | *string*           | :heavy_minus_sign: | N/A                |
| `revokedAt`        | *string*           | :heavy_minus_sign: | N/A                |