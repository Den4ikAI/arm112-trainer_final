-- Базовые сущности: службы, пользователи.
-- ДДС — это организация, человек к ней привязан через users.service_id.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE services (
    id            text PRIMARY KEY,
    title         text NOT NULL,
    is_primary    boolean NOT NULL DEFAULT false,
    external_code text,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TYPE user_role AS ENUM ('student', 'teacher', 'admin');

CREATE TABLE users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    login         text UNIQUE NOT NULL,
    password_hash text NOT NULL,
    fio           text NOT NULL,
    role          user_role NOT NULL,
    -- NULL для преподавателя и администратора: они не привязаны к ДДС
    service_id    text REFERENCES services(id),
    workstation   text,
    is_active     boolean NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX users_service_idx ON users (service_id) WHERE is_active;
