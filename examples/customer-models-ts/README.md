# Customer models (TypeScript)

This stack lets each enterprise customer connect models from its own AWS or
Google Cloud account. It creates one frozen `alien.AI` resource and makes that
resource available through Remote Bindings.

Release the stack from this directory:

```bash
alien release
```

Create a customer setup link for the release and send it to the customer's cloud
administrator. After setup completes, application code calls the Alien AI Gateway
with an ordinary project API key and the customer's stable external ID:

```bash
curl https://ai.alien.dev/v1/models \
  -H "Authorization: Bearer $ALIEN_API_KEY" \
  -H "X-Alien-External-ID: enterprise-123"
```

The same base URL and headers work with supported OpenAI-compatible requests.
Model availability comes from the connected provider account and location; it
does not promise unused quota at the instant of a request.
