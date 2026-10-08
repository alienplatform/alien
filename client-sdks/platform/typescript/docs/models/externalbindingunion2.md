# ExternalBindingUnion2

Binding parameters for Queue at runtime or in templates.


## Supported Types

### `models.ExternalBindingSqs`

```typescript
const value: models.ExternalBindingSqs = {
  queueUrl: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "sqs",
  type: "queue",
};
```

### `models.ExternalBindingPubsub`

```typescript
const value: models.ExternalBindingPubsub = {
  subscription: "<value>",
  topic: "<value>",
  service: "pubsub",
  type: "queue",
};
```

### `models.ExternalBindingServicebus`

```typescript
const value: models.ExternalBindingServicebus = {
  namespace: "<value>",
  queueName: "<value>",
  service: "servicebus",
  type: "queue",
};
```

### `models.ExternalBindingLocalQueue`

```typescript
const value: models.ExternalBindingLocalQueue = {
  queuePath: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "local-queue",
  type: "queue",
};
```
