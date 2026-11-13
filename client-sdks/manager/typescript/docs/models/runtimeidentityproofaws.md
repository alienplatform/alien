# RuntimeIdentityProofAws

An audience-signed STS GetCallerIdentity request.

## Example Usage

```typescript
import { RuntimeIdentityProofAws } from "@alienplatform/manager-api/models";

let value: RuntimeIdentityProofAws = {
  presignedUrl: "https://lumbering-clavicle.com",
  type: "aws",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `presignedUrl`     | *string*           | :heavy_check_mark: | N/A                |
| `type`             | *"aws"*            | :heavy_check_mark: | N/A                |
