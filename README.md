# diridp

Diridp is an OpenID Connect identity provider that issues tokens (JWTs) as
regular files on the local filesystem.

The tokens generated by diridp are typically used by other processes on the
same machine to identify themselves to some third party service. Because diridp
rotates all signing keys and tokens, these can replace otherwise permanent
credentials that would be used instead.

## Usage

- See [releases] for binaries
- [Example systemd unit](./extra/diridp.service)
- [NixOS module](./nix/README.md)

[releases]: https://github.com/stephank/diridp/releases

A minimal config looks like:

```yaml
providers:
  - issuer: "https://example.com"
    keys:
      - alg: EdDSA
        crv: Ed25519
    tokens:
      - path: "/run/diridp/my-application/token"
        claims:
          sub: "my-application"
          aud: "some-cloud-service.example.com"
```

See the [configuration template](./diridp.dist.yaml) for all available options.

Diridp is built to require no network access at all. Serving the OpenID Connect
documents is left to an external process like Nginx or Apache httpd. Typically,
you'd configure an HTTPS virtual host to serve from
`/var/lib/diridp/<provider>/webroot`. In Nginx, this may look like:

```nginx
server {
  listen 0.0.0.0:443 ssl;
  server_name example.com;
  ssl_certificate /etc/nginx/ssl/example.com/fullchain.pem;
  ssl_certificate_key /etc/nginx/ssl/example.com/privkey.pem;

  # All files in this root are JSON, but may not have the extension.
  root /var/lib/diridp/main/webroot;
  types { }
  default_type application/json;

  # This is a decent default matching diridp config defaults. A good rule of
  # thumb when adjusting this: `max-age + 3 * s-maxage <= key_publish_margin`
  add_header cache-control "public, max-age=3600, s-maxage=600";
}
```

If you'd like to serve other content from the same vhost, you may also
configure locations for the two specific files that need to be served:

```nginx
location = /.well-known/openid-configuration {
  root /var/lib/diridp/main/webroot;
  types { }
  default_type application/json;
  add_header cache-control "public, max-age=3600, s-maxage=600";
}

# This path can be customized with the `jwks_path` option in diridp config.
location = /jwks.json {
  root /var/lib/diridp/main/webroot;
  types { }
  default_type application/json;
  add_header cache-control "public, max-age=3600, s-maxage=600";
}
```

## Example: AWS integration

An identity provider can be created in AWS IAM. This instructs AWS how to
verify tokens. In Terraform syntax, this'd look like:

```hcl
locals {
  example_idp_host = "example.com"
}

resource "aws_iam_openid_connect_provider" "example_idp" {
  # This must match the `issuer` setting in diridp, and virtual host.
  url = "https://${local.example_idp_host}"

  # This must match the `aud` claim in the diridp token.
  client_id_list = ["sts.amazonaws.com"]

  # This pins the certificate authority (CA) that issued the HTTPS certificate,
  # and is required by AWS. Here we determine it at `terraform apply` time, so
  # if the CA certificate ever changes, simply re-apply the Terraform config.
  # This requires the `hashicorp/tls` provider.
  thumbprint_list = [data.tls_certificate.example_idp.certificates[0].sha1_fingerprint]
}

data "tls_certificate" "example_idp" {
  url = "https://${local.example_idp_host}"
}
```

Once configured, AWS will accept tokens from this identity provider in calls to
the STS `AssumeRoleWithWebIdentity` action, but we first need to create a role
that can be assumed with the token:

```hcl
resource "aws_iam_role" "example_role" {
  name = "example_role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = "sts:AssumeRoleWithWebIdentity"
        Principal = {
          Federated = aws_iam_openid_connect_provider.example_idp.arn
        }
        Condition = {
          StringLike = {
            # Limit this role to tokens with matching `sub` and `aud` claims.
            "${local.example_idp_host}:sub" = "my-application"
            "${local.example_idp_host}:aud" = "sts.amazonaws.com"
          }
        }
      }
    ]
  })
}
```

(The above example role has no permissions at all. You'd typically attach some
policies to it to allow it to actually do anything.)

Now, you can use the role in any application using the offical AWS SDKs by
setting these environment variables:

```bash
# Matches the `path` in diridp configuration.
AWS_WEB_IDENTITY_TOKEN_FILE="/run/diridp/my-application/token"
# Example, replace with the actual role ARN.
AWS_ROLE_ARN="arn:aws:iam::123456789:role/example_role"
```

Behind the scenes, the AWS SDK does something similar to the following AWS CLI
command:

```bash
aws sts assume-role-with-web-identity \
  --role-arn "arn:aws:iam::123456789:role/example_role" \
  --role-session-name "<generated by the SDK>" \
  --web-identity-token "<token contents>"
```

This effectively creates a regular AWS access key with a very short lifetime (1
hour by default) and then continues as normal using that access key. But the
AWS SDK handles automatic refresh for you.

## Example: Docker integration

By using a path with a parameter, new tokens can be defined at run-time simply
by creating directories:

```yaml
providers:
  - issuer: "https://example.com"
    keys:
      - alg: EdDSA
        crv: Ed25519
    tokens:
      - path: "/run/diridp/containers/:sub/aws_token"
        claims:
          aud: "sts.amazonaws.com"
```

Docker automatically creates directories for volume mounts, so an application
using the AWS SDK could be started as follows:

```bash
docker run \
  -v /run/diridp/containers/my_app:/run/secrets/diridp:ro \
  -e AWS_WEB_IDENTITY_TOKEN_FILE=/run/secrets/diridp/aws_token \
  -e AWS_ROLE_ARN=arn:aws:iam::123456789:role/example_role \
  my_app:latest
```

Note that when stopping / removing containers, these directories are not
automatically cleaned up.
