-- Opaque bearer tokens for the JSON REST API, used instead of the cookie
-- session that the HTML/htmx UI relies on. Issued on `POST /api/v1/login`,
-- checked on every other `/api/v1/*` and `/admin/api/v1/*` request via the
-- `Authorization: Bearer <token>` header, and deleted on `POST
-- /api/v1/logout`.
create table api_tokens (
    token varchar(64) not null primary key,
    user_id integer not null references users(id),
    created_at datetime not null,
    expires_at datetime not null
);

create index api_tokens_user_id_idx on api_tokens(user_id);
