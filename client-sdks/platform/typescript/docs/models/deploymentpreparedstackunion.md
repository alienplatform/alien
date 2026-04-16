# DeploymentPreparedStackUnion


## Supported Types

### `models.DeploymentPreparedStack`

```typescript
const value: models.DeploymentPreparedStack = {
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
      lifecycle: "live-on-setup",
    },
  },
};
```

### `any`

```typescript
const value: any = "<value>";
```

