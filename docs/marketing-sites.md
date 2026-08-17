---
publish: false
---

# Marketing sites — publishing the brand sites as static files

The brand marketing site is not Navigator. It is a React build that ships as files: no server, no database, no cluster,
no request-time rendering. It lives in its own repository, publishes to its own GCP project, and can be deployed,
broken, or rebuilt without touching a deployment that serves clients.

| Site | Repository | Bucket | Hostname |
| --- | --- | --- | --- |
| Neon Law Foundation | `marketing/neon-law-foundation` | `neon-marketing-foundation` | `www.neonlaw.com` |

One site. The registry above is the whole of it: one repository, one bucket, one hostname, all Neon Law.

One archive bucket sits outside that registry, holding the published objects of an earlier marketing site as the only
surviving copy of that content. It is an archive to read: nothing routes to it, nothing publishes to it, and `ops gcp
marketing setup` neither creates nor reports it. Leave it in place and treat the registry above as the live inventory.

The Foundation site holds **`www.neonlaw.com`**, and that name is promised to the `neon-law-prod` deployment — see
[`environments`](environments.md). It is the one site in this registry with a scheduled end: when the deployment takes
the hostname, this site is retired rather than moved, and `neonlaw.com` becomes a 301 to the same name. Until then the
bucket and load balancer below are what answers there.

## Why this is its own project

`neon-marketing` holds published marketing content and nothing else. Its buckets are **anonymously readable by design**,
which is exactly why it must never grow an application: a documents bucket or a cluster in a project built around public
reads is a disclosure bug, not a misconfiguration.

That is enforced in code rather than by convention. `TenantRole::Marketing` joins the tenant registry in
`cli/src/devx/gcp/tenants.rs`, so `ops gcp setup` refuses `neon-marketing` and `ops gcp marketing setup` refuses an
environment or the hub — each before the first GCP call. A dry-run test asserts a marketing run touches no cluster,
network, or documents bucket.

## Provisioning

```bash
navigator ops gcp marketing setup --dry-run   # read the plan first
navigator ops gcp marketing setup
```

Idempotent: every step treats "already exists" as success, so a re-run after a partial failure converges. It needs
Application Default Credentials (`gcloud auth application-default login`), which is a **separate** credential from
`gcloud auth login` and has no fallback to the gcloud CLI session.

Per site it creates the website bucket, the deployer identity, the load balancer, and the trust between them:

- The **website bucket** — uniform bucket-level access, `MainPageSuffix: index.html`, `NotFoundPage: 404.html`, and an
  `allUsers` `objectViewer` grant.
- The **deployer service account**, bound on the bucket to `objectAdmin` and `legacyBucketReader`, and to nothing at the
  project level, so it can publish its own site and reach nothing else.
- The **load balancer** — a reserved global address, a CDN-backed backend bucket, a URL map, an HTTPS proxy and
  forwarding rule, plus a port-80 chain that 301s to HTTPS.
- The **certificate chain** — a DNS authorization, the certificate it validates, and the certificate map the HTTPS proxy
  points at.
- The **Workload Identity binding** that lets the site's repository impersonate its deployer.

The shared identity pool and the GHE provider are created once, before the per-site loop.

### A bucket alone cannot do this

`storage.googleapis.com` has its own certificate; `www.neonlaw.com` needs one issued for that name, and GCS has no way
to hold it. A static site on a real hostname is therefore always a bucket **plus** a load balancer. The load balancer
also requires the objects to be publicly readable — it fetches them anonymously, and there is no service-account path
for a backend bucket.

### The org policy exception

The organization enforces domain-restricted sharing (`constraints/iam.allowedPolicyMemberDomains`), which rejects
`allUsers` in any IAM binding. The legacy constraint cannot take `allUsers` as an allowed value, so `neon-marketing`
carries a **project-scoped** override:

```yaml
# policy.yaml
constraint: constraints/iam.allowedPolicyMemberDomains
listPolicy:
  allValues: ALLOW
```

```bash
gcloud resource-manager org-policies set-policy --project neon-marketing policy.yaml
```

The org-wide constraint is untouched and every other project still inherits it. To remove the exception:

```bash
gcloud resource-manager org-policies delete iam.allowedPolicyMemberDomains --project neon-marketing
```

## Deploying

Each repository's `.github/workflows/deploy.yml` publishes on every push to `main` — in practice once per merged pull
request, against a tree the `verify` job already typechecked, linted, and built.

### Authentication is keyless

The job mints a short-lived OIDC token and federates it into the deployer. **No service-account key exists**, so there
is none to leak or rotate.

The issuer is the subtlety. These repositories are on GitHub Enterprise Cloud with data residency, which issues Actions
tokens from the enterprise's own subdomain:

```text
https://token.actions.github.com
```

That is **not** `token.actions.githubusercontent.com`, which is what `ops gcp hub setup` trusts for the Navigator
repository on github.com. The two live under the same identity pool as separate providers, `ghe-oidc` and `github-oidc`,
precisely because they trust different issuers. Collapsing them would have one silently overwrite the other, and a
provider carrying the wrong issuer rejects every token with a signature failure that never names the mistake.

Because the issuer already sits on a per-enterprise subdomain it is inherently scoped to this enterprise. No
`include_enterprise_slug` configuration is required — that setting exists for enterprises hosted on github.com, which
share one issuer. The provider narrows further to the `marketing` owner, and each impersonation binding narrows again to
one `attribute.repository`, so neither marketing repository can publish over the other's site.

Verify the issuer before changing the constant:

```bash
curl https://token.actions.github.com/.well-known/openid-configuration
```

### The repository variables

Three per repository, **variables and not secrets** — a provider resource name, a service-account email, and a bucket
name are public identifiers, and the trust is enforced on Google's side by the Workload Identity binding.

| Variable | Value |
| --- | --- |
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | the `ghe-oidc` provider resource, printed by the setup command |
| `GCP_SERVICE_ACCOUNT` | `<slug>-deployer@neon-marketing.iam.gserviceaccount.com` |
| `GCS_BUCKET` | `neon-marketing-<slug>` |

The provider resource is the same for every site:

```text
projects/93609063550/locations/global/workloadIdentityPools/github/providers/ghe-oidc
```

### Upload order is load-bearing

```bash
# 1. hashed assets, immutable, nothing deleted
gcloud storage rsync dist/assets "gs://$BUCKET/assets" --recursive \
    --cache-control="public, max-age=31536000, immutable"

# 2. HTML and static files, revalidated, pruned
gcloud storage rsync dist "gs://$BUCKET" --recursive \
    --exclude="^assets/" --delete-unmatched-destination-objects \
    --cache-control="no-cache, must-revalidate"
```

Assets go first, so by the time any HTML naming a new hashed filename is readable, that file already exists. The reverse
order serves pages whose assets `404` for the length of the second upload.

Assets are never deleted. Their names are content-hashed, so a stale one is unreachable rather than wrong, and keeping
it means a visitor part-way through loading the previous release does not lose its stylesheet. That is what makes
`immutable` safe on assets and `no-cache` correct for HTML.

The `--exclude` pattern keeps the pruning pass from deleting the previous release's assets. That behaviour was verified
against a real bucket rather than inferred from the flag documentation.

Because caching is delegated to each object's own headers (`cdnPolicy.cacheMode: USE_ORIGIN_HEADERS`), a deploy never
needs a CDN invalidation — which is why the deployer is deliberately **not** granted `compute.urlMaps.invalidateCache`.

### The deployer's permissions

`objectAdmin` alone is not enough. It covers objects but not the bucket resource, and `gcloud storage rsync` reads the
destination bucket's metadata. The gap is invisible on a sync into a prefix and fails only on a sync to the bucket root,
so a deploy will upload the assets and then die on `storage.buckets.get denied` for the HTML. `legacyBucketReader`
supplies `buckets.get` and object listing without granting any write on the bucket itself.

## The DNS cutover

Two records per hostname, and **the order is the whole point**.

The certificate is issued through Certificate Manager with a **DNS authorization**, not by a CA calling the load
balancer. So it can reach `ACTIVE` while the hostname still resolves to whatever serves it today, and the cutover
carries no TLS gap.

1. **Publish the `CNAME`.** `ops gcp marketing setup` prints one per site. It proves domain control, and it must stay
   published for renewal to keep working.
2. **Wait for `ACTIVE`.** Minutes, usually. Nothing user-visible changes.
3. **Move the `A` record** to the load balancer address. HTTPS works from the first request.

Moving the `A` record before the certificate is `ACTIVE` inverts the benefit and takes the hostname down for the length
of issuance.

```bash
navigator ops gcp marketing setup            # prints both records and the certificate state

gcloud certificate-manager certificates list --project neon-marketing \
    --format="table(name,san_dnsnames,state)"
gcloud compute addresses list --global --project neon-marketing
```

`neonlaw.org` sits in DNSimple account `176770`, while every other Navigator zone sits under `174981`. A command aimed
at the wrong account fails with `Zone not found`, which reads like a typo in the domain rather than a permissions error,
so set `DNS_ACCT` per zone rather than once per session.

Use the `ops dns setup` command in [`dns`](dns.md) for the `A` record, one run per zone. It is additive and refuses to
replace a conflicting `CNAME` or `URL` record — that deletion is a reviewed operator action, because it takes the
current site off the air.

## Verifying

```bash
# objects landed and are anonymously readable
curl -sI https://storage.googleapis.com/neon-marketing-foundation/index.html

# the two cache policies
curl -sI "https://storage.googleapis.com/$BUCKET/index.html" | grep -i cache-control
curl -sI "https://storage.googleapis.com/$BUCKET/assets/$HASHED_JS" | grep -i cache-control

# after a DNS cutover, against whichever hostname the site is given
curl -sI "http://$HOST"                  # 301 to https
curl -sI "https://$HOST"                 # 200
curl -sI "https://$HOST/nothing-here"    # 404, the branded page
```

## Related

- [`environments`](environments.md) — the runtime projects, and the deployment that inherits `www.neonlaw.com`.
- [`dns`](dns.md) — the record model and the `ops dns setup` command.
- [`gitops`](gitops.md) — branch, PR, and merge flow.
