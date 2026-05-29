# KubernetesOwnerReference

## Example Usage

```typescript
import { KubernetesOwnerReference } from "@alienplatform/manager-api/models";

let value: KubernetesOwnerReference = {
  controller: true,
  kind: "<value>",
  name: "<value>",
  uid: "<id>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `controller`       | *boolean*          | :heavy_check_mark: | N/A                |
| `kind`             | *string*           | :heavy_check_mark: | N/A                |
| `name`             | *string*           | :heavy_check_mark: | N/A                |
| `uid`              | *string*           | :heavy_check_mark: | N/A                |