# Taskflow API

Base path: `/tasks`

## Operations

| Method | Path | Operation |
|--------|------|-----------|
| GET | `/tasks/` | listTasks |
| POST | `/tasks/` | createTask |
| DELETE | `/tasks/{id}` | deleteTask |
| GET | `/tasks/{id}` | getTask |
| PUT | `/tasks/{id}` | updateTask |

## Schemas

- `Assignee` (object)
- `CreateTaskRequest` (object)
- `ErrorResponse` (object)
- `Status` (enum)
- `Task` (object)
- `TaskList` (object)
- `UpdateTaskRequest` (object)
