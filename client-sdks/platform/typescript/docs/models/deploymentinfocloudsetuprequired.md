# DeploymentInfoCloudSetupRequired

Set when this release changes something a chart upgrade cannot apply: the install needs its cloud setup run first.

## Example Usage

```typescript
import { DeploymentInfoCloudSetupRequired } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCloudSetupRequired = {
  reason: "<value>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `reason`           | *string*           | :heavy_check_mark: | N/A                |