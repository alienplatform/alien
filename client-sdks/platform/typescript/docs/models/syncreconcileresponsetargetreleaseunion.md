# SyncReconcileResponseTargetReleaseUnion


## Supported Types

### `models.SyncReconcileResponseTargetRelease`

```typescript
const value: models.SyncReconcileResponseTargetRelease = {
  releaseId: "<id>",
  stack: {
    id: "<id>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        dependencies: [
          {
            id: "<id>",
            type: "<value>",
          },
        ],
        lifecycle: "live",
      },
    },
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

