# ExternalBindingFirestore

GCP Firestore KV binding configuration

## Example Usage

```typescript
import { ExternalBindingFirestore } from "@alienplatform/platform-api/models";

let value: ExternalBindingFirestore = {
  collectionName: "<value>",
  databaseId: "<id>",
  projectId: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "firestore",
  type: "kv",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `collectionName`                                                                                                     | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `databaseId`                                                                                                         | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `projectId`                                                                                                          | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"firestore"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeKv2](../models/typekv2.md)                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
