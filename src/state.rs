// state.rs

// *************************************************************************
// * Copyright (C) 2017-2018 Daniel Mueller (deso@posteo.net)              *
// *                                                                       *
// * This program is free software: you can redistribute it and/or modify  *
// * it under the terms of the GNU General Public License as published by  *
// * the Free Software Foundation, either version 3 of the License, or     *
// * (at your option) any later version.                                   *
// *                                                                       *
// * This program is distributed in the hope that it will be useful,       *
// * but WITHOUT ANY WARRANTY; without even the implied warranty of        *
// * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the         *
// * GNU General Public License for more details.                          *
// *                                                                       *
// * You should have received a copy of the GNU General Public License     *
// * along with this program.  If not, see <http://www.gnu.org/licenses/>. *
// *************************************************************************

use std::cell::RefCell;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Result;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use serde::Deserialize;
use serde::Serialize;
use serde_json::from_reader;
use serde_json::to_string_pretty as to_json;

use query::Query;
use query::QueryBuilder;
use ser::state::ProgState as SerProgState;
use ser::state::TaskState as SerTaskState;
use tags::Tag;
use tags::Templates;
use tasks::Id as TaskId;
use tasks::Task;
use tasks::Tasks;


/// An object encapsulating the program's relevant state.
#[derive(Debug)]
pub struct State {
  prog_path: PathBuf,
  task_path: PathBuf,
  templates: Rc<Templates>,
  queries: Vec<Query>,
  tasks: Rc<RefCell<Tasks>>,
}

impl State {
  /// Create a new `State` object, loaded from files.
  pub fn new<P>(prog_path: P, task_path: P) -> Result<Self>
  where
    P: Into<PathBuf> + AsRef<Path>,
  {
    let prog_state = Self::load_state::<SerProgState>(prog_path.as_ref())?;
    let task_state = Self::load_state::<SerTaskState>(task_path.as_ref())?;

    Self::with_serde(prog_state, prog_path, task_state, task_path)
  }

  /// Create a new `State` object from a serializable one.
  pub fn with_serde<P>(mut prog_state: SerProgState, prog_path: P,
                           task_state: SerTaskState, task_path: P) -> Result<Self>
  where
    P: Into<PathBuf>,
  {
    let (templates, map) = Templates::with_serde(task_state.templates);
    let templates = Rc::new(templates);
    let tasks = Tasks::with_serde(task_state.tasks, templates.clone(), &map)?;
    let tasks = Rc::new(RefCell::new(tasks));
    let mut queries = vec![
      QueryBuilder::new(tasks.clone()).build("all"),
    ];
    for query in prog_state.queries.drain(..) {
      queries.push(Query::with_serde(query, &templates, &map, tasks.clone())?)
    }

    Ok(State {
      prog_path: prog_path.into(),
      task_path: task_path.into(),
      templates: templates,
      queries: queries,
      tasks: tasks,
    })
  }

  /// Load some serialized state from a file.
  fn load_state<T>(path: &Path) -> Result<T>
  where
    T: Default,
    for<'de> T: Deserialize<'de>,
  {
    match File::open(&path) {
      Ok(file) => Ok(from_reader::<File, T>(file)?),
      Err(e) => {
        // If the file does not exist we create an empty object and work
        // with that.
        if e.kind() == ErrorKind::NotFound {
          Ok(Default::default())
        } else {
          Err(e)
        }
      },
    }
  }

  /// Convert this object into a serializable one.
  fn to_serde(&self) -> (SerProgState, SerTaskState) {
    // The first query is the "all" query which we always create
    // implicitly and never persist.
    let queries = self
      .queries
      .iter()
      .skip(1)
      .map(|x| x.to_serde())
      .collect();

    let task_state = SerTaskState {
      templates: self.templates.to_serde(),
      tasks: self.tasks.borrow().to_serde(),
    };
    let program_state = SerProgState {
      queries: queries,
    };

    (program_state, task_state)
  }

  /// Persist the state into a file.
  pub fn save(&self) -> Result<()> {
    let (prog_state, task_state) = self.to_serde();
    Self::save_state(&self.prog_path, prog_state)?;
    // TODO: We risk data inconsistencies if the second save operation
    //       fails.
    Self::save_state(&self.task_path, task_state)?;
    Ok(())
  }

  /// Save some state into a file.
  fn save_state<T>(path: &Path, state: T) -> Result<()>
  where
    T: Serialize,
  {
    let serialized = to_json(&state)?;
    OpenOptions::new()
      .create(true)
      .truncate(true)
      .write(true)
      .open(path)?
      .write_all(serialized.as_ref())?;
    Ok(())
  }

  /// Retrieve the tasks associated with this `State` object.
  #[cfg(test)]
  pub fn tasks(&self) -> Vec<Task> {
    self.tasks.borrow().iter().cloned().collect()
  }

  /// Retrieve the queries to use.
  pub fn queries(&self) -> impl Iterator<Item=&Query> {
    self.queries.iter()
  }

  /// Add a new task to the list of tasks.
  pub fn add_task(&self, summary: String, tags: Vec<Tag>) -> TaskId {
    self.tasks.borrow_mut().add(summary, tags)
  }

  /// Remove the task with the given `TaskId`.
  pub fn remove_task(&self, id: TaskId) {
    self.tasks.borrow_mut().remove(id)
  }

  /// Update a task.
  pub fn update_task(&self, task: Task) {
    self.tasks.borrow_mut().update(task)
  }
}


#[cfg(test)]
pub mod tests {
  use super::*;

  use ser::tags::Id as SerId;
  use ser::tags::Tag as SerTag;
  use ser::tags::Template as SerTemplate;
  use ser::tags::Templates as SerTemplates;
  use ser::tasks::Task as SerTask;
  use ser::tasks::Tasks as SerTasks;
  use test::make_tasks;
  use test::NamedTempFile;


  /// Create a state object based off of two temporary configuration files.
  fn make_state(count: usize) -> (State, NamedTempFile, NamedTempFile) {
    let prog_state = Default::default();
    let task_state = SerTaskState {
      templates: Default::default(),
      tasks: SerTasks(make_tasks(count)),
    };
    let prog_file = NamedTempFile::new();
    let task_file = NamedTempFile::new();
    let state = State::with_serde(prog_state, prog_file.path(), task_state, task_file.path());
    (state.unwrap(), prog_file, task_file)
  }

  #[test]
  fn save_and_load_state() {
    let (state, prog_file, task_file) = make_state(3);
    state.save().unwrap();

    let new_state = State::new(prog_file.path(), task_file.path()).unwrap();
    let new_task_vec = new_state
      .tasks
      .borrow()
      .iter()
      .map(|x| x.to_serde())
      .collect::<Vec<_>>();
    assert_eq!(new_task_vec, make_tasks(3));
  }

  #[test]
  fn load_state_file_not_found() {
    let (prog_path, task_path) = {
      let (state, prog_file, task_file) = make_state(1);
      state.save().unwrap();

      (prog_file.path().clone(), task_file.path().clone())
    };

    // The files are removed by now, so we can test that `State` handles
    // such missing files gracefully.
    let new_state = State::new(prog_path, task_path).unwrap();
    let new_task_vec = new_state
      .tasks
      .borrow()
      .iter()
      .map(|x| x.to_serde())
      .collect::<Vec<_>>();
    assert_eq!(new_task_vec, make_tasks(0));
  }

  #[test]
  fn load_state_with_invalid_tag() {
    let prog_state = Default::default();
    let prog_path = PathBuf::default();
    let templates = SerTemplates(Default::default());
    let tasks = SerTasks(vec![
      SerTask {
        summary: "a task!".to_string(),
        tags: vec![
          SerTag {
            id: SerId::new(42),
          },
        ],
      },
    ]);
    let task_state = SerTaskState {
      templates: templates,
      tasks: tasks,
    };
    let task_path = PathBuf::default();

    let err = State::with_serde(prog_state, prog_path, task_state, task_path).unwrap_err();
    assert_eq!(err.to_string(), "Encountered invalid tag Id 42")
  }

  #[test]
  fn load_state() {
    let prog_state = Default::default();
    let prog_path = PathBuf::default();

    let id_tag1 = SerId::new(29);
    let id_tag2 = SerId::new(1337 + 42 - 1);

    let templates = SerTemplates(vec![
      SerTemplate {
        id: id_tag1,
        name: "tag1".to_string(),
      },
      SerTemplate {
        id: id_tag2,
        name: "tag2".to_string(),
      },
    ]);

    let tasks = SerTasks(vec![
      SerTask {
        summary: "a task!".to_string(),
        tags: vec![
          SerTag {
            id: id_tag2,
          },
        ],
      },
      SerTask {
        summary: "an untagged task".to_string(),
        tags: Default::default(),
      },
      SerTask {
        summary: "a tag1 task".to_string(),
        tags: vec![
          SerTag {
            id: id_tag1,
          },
        ],
      },
      SerTask {
        summary: "a doubly tagged task".to_string(),
        tags: vec![
          SerTag {
            id: id_tag2,
          },
          SerTag {
            id: id_tag1,
          },
        ],
      },
    ]);
    let task_state = SerTaskState {
      templates: templates,
      tasks: tasks,
    };
    let task_path = PathBuf::default();

    let state = State::with_serde(prog_state, prog_path, task_state, task_path).unwrap();
    let tasks = state.tasks.borrow();
    let mut it = tasks.iter();

    let task1 = it.next().unwrap();
    let mut tags = task1.tags();
    assert_eq!(tags.next().unwrap().name(), "tag2");
    assert!(tags.next().is_none());

    let task2 = it.next().unwrap();
    assert!(task2.tags().next().is_none());

    let task3 = it.next().unwrap();
    let mut tags = task3.tags();
    assert_eq!(tags.next().unwrap().name(), "tag1");
    assert!(tags.next().is_none());

    let task4 = it.next().unwrap();
    let mut tags = task4.tags();
    assert!(tags.next().is_some());
    assert!(tags.next().is_some());
    assert!(tags.next().is_none());
  }
}