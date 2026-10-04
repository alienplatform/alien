# RemoteFirestoreKvBinding

Concrete Firestore KV topology returned to remote clients.

## Example Usage

```typescript
import { RemoteFirestoreKvBinding } from "@alienplatform/manager-api/models";

let value: RemoteFirestoreKvBinding = {
  collectionName: "<value>",
  databaseId: "<id>",
  projectId: "<id>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `collectionName`                                   | *string*                                           | :heavy_check_mark:                                 | Firestore collection holding this store's entries. |
| `databaseId`                                       | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `projectId`                                        | *string*                                           | :heavy_check_mark:                                 | N/A                                                |