# RuntimeIdentityProofAzure

A managed-identity token for this manager's audience.

## Example Usage

```typescript
import { RuntimeIdentityProofAzure } from "@alienplatform/manager-api/models";

let value: RuntimeIdentityProofAzure = {
  accessToken: "<value>",
  type: "azure",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `accessToken`      | *string*           | :heavy_check_mark: | N/A                |
| `type`             | *"azure"*          | :heavy_check_mark: | N/A                |
