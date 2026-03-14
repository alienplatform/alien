# EnvironmentInfoAws

AWS environment information

## Example Usage

```typescript
import { EnvironmentInfoAws } from "@alienplatform/manager-api/models";

let value: EnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `accountId`        | *string*           | :heavy_check_mark: | AWS account ID     |
| `region`           | *string*           | :heavy_check_mark: | AWS region         |
| `platform`         | *"aws"*            | :heavy_check_mark: | N/A                |