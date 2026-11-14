-- Make Your Life Easier: Password Manager sync.
--
-- Run once in the Supabase dashboard: SQL Editor > New query > paste > Run.
-- Safe to run again.
--
-- The server only ever stores ciphertext. Every entry is encrypted on the
-- PC with a key that only the user's master password (or recovery code)
-- unlocks; titles, addresses and user names are encrypted too. Row level
-- security lets each signed-in user reach their own rows only.

-- The vault's header: which vault it is, how the master key is derived,
-- and the vault key wrapped by the master password and the recovery code.
create table if not exists public.password_vault (
  user_id uuid primary key references auth.users (id) on delete cascade,
  vault_id uuid not null,
  kdf jsonb not null,
  wrapped_key text not null,
  recovery_wrapped_key text not null,
  updated_at timestamptz not null default now()
);

-- The header sealed on the device with the vault key: an AEAD over vault_id,
-- kdf and both wraps. A device takes in a changed header (a new master
-- password) by itself only if its seal opens with the vault key, so the
-- account alone cannot replace the wraps. Null from builds before seals, and
-- until a device that has the key next opens the vault and sends one.
alter table public.password_vault
  add column if not exists header_seal text
    check (header_seal is null or length(header_seal) <= 1024);

-- One row per entry. A deleted entry stays as a row with deleted = true
-- and no ciphertext, so the deletion reaches every PC.
create table if not exists public.password_items (
  user_id uuid not null references auth.users (id) on delete cascade,
  id uuid not null,
  revision bigint not null check (revision > 0),
  deleted boolean not null default false,
  ciphertext text not null default '' check (length(ciphertext) <= 262144),
  updated_at bigint not null,
  primary key (user_id, id)
);

alter table public.password_vault enable row level security;
alter table public.password_items enable row level security;

drop policy if exists "own password vault" on public.password_vault;
create policy "own password vault" on public.password_vault
  for all to authenticated
  using ((select auth.uid()) = user_id)
  with check ((select auth.uid()) = user_id);

drop policy if exists "own password items" on public.password_items;
create policy "own password items" on public.password_items
  for all to authenticated
  using ((select auth.uid()) = user_id)
  with check ((select auth.uid()) = user_id);

-- A row only moves forward: an update must raise the revision. With the
-- app's compare-and-swap (update ... where revision = the one it saw) two
-- PCs can never overwrite each other's change.
create or replace function public.password_items_forward_only()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.revision <= old.revision then
    raise exception 'stale password item revision' using errcode = '40001';
  end if;
  return new;
end;
$$;

drop trigger if exists password_items_forward_only on public.password_items;
create trigger password_items_forward_only
  before update on public.password_items
  for each row execute function public.password_items_forward_only();

-- The header's timestamp follows its changes.
create or replace function public.password_vault_touch()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

drop trigger if exists password_vault_touch on public.password_vault;
create trigger password_vault_touch
  before update on public.password_vault
  for each row execute function public.password_vault_touch();
