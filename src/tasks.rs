// tasks.rs

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

use std::cmp::PartialEq;
use std::collections::BTreeMap;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::rc::Rc;
use std::slice;

use id::Id as IdT;
use ser::tasks::Task as SerTask;
use ser::tasks::Tasks as SerTasks;
use tags::Id as TagId;
use tags::Tag;
use tags::TagMap;
use tags::Templates;

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct T(());

pub type Id = IdT<T>;


/// A struct representing a task item.
#[derive(Clone, Debug)]
pub struct Task {
  id: Id,
  pub summary: String,
  tags: BTreeMap<TagId, Tag>,
  templates: Rc<Templates>,
}

impl Task {
  /// Create a new task.
  #[cfg(test)]
  pub fn new(summary: impl Into<String>) -> Self {
    Task {
      id: Id::new(),
      summary: summary.into(),
      tags: Default::default(),
      templates: Rc::new(Templates::new()),
    }
  }

  /// Create a task using the given summary.
  fn with_summary_and_tags(summary: String, mut tags: Vec<Tag>, templates: Rc<Templates>) -> Self {
    Task {
      id: Id::new(),
      summary: summary,
      tags: tags.drain(..).map(|x| (x.id(), x)).collect(),
      templates: templates,
    }
  }

  /// Create a new task from a serializable one.
  fn with_serde(mut task: SerTask, templates: Rc<Templates>, map: &TagMap) -> Result<Task> {
    let mut tags = BTreeMap::new();
    for tag in task.tags.drain(..) {
      let id = map.get(&tag.id).ok_or_else(|| {
        let error = format!("Encountered invalid tag Id {}", tag.id);
        Error::new(ErrorKind::InvalidInput, error)
      })?;
      tags.insert(*id, templates.instantiate(*id));
    }

    Ok(Task {
      id: Id::new(),
      summary: task.summary,
      tags: tags,
      templates: templates,
    })
  }

  /// Convert this task into a serializable one.
  pub fn to_serde(&self) -> SerTask {
    SerTask {
      summary: self.summary.clone(),
      tags: self.tags.iter().map(|(_, x)| x.to_serde()).collect(),
    }
  }

  /// Retrieve this task's `Id`.
  pub fn id(&self) -> Id {
    self.id
  }

  /// Retrieve an iterator over this task's tags.
  pub fn tags(&self) -> impl Iterator<Item=&Tag> + Clone {
    self.tags.values()
  }

  /// Check whether the task is tagged as complete or not.
  pub fn is_complete(&self) -> bool {
    let id = self.templates.complete_tag().id();
    self.tags.contains_key(&id)
  }

  /// Toggle the completion state of the task.
  pub fn toggle_complete(&mut self) {
    let id = self.templates.complete_tag().id();

    // Try removing the complete tag, if that succeeds we are done (as
    // the tag was present and got removed), otherwise insert it (as it
    // was not present).
    if self.tags.remove(&id).is_none() {
      let tag = self.templates.instantiate(id);
      self.tags.insert(id, tag);
    }
  }
}

impl PartialEq for Task {
  fn eq(&self, other: &Task) -> bool {
    let result = self.id == other.id;
    assert!(!result || self.summary == other.summary);
    assert!(!result || self.tags == other.tags);
    result
  }
}


pub type TaskIter<'a> = slice::Iter<'a, Task>;


/// A management struct for tasks and their associated data.
#[derive(Debug, PartialEq)]
pub struct Tasks {
  templates: Rc<Templates>,
  tasks: Vec<Task>,
}

impl Tasks {
  /// Create a new `Tasks` object from a serializable one.
  pub fn with_serde(mut tasks: SerTasks, templates: Rc<Templates>, map: &TagMap) -> Result<Self> {
    let mut new_tasks = Vec::with_capacity(tasks.0.len());
    for task in tasks.0.drain(..) {
      let task = Task::with_serde(task, templates.clone(), &map)?;
      new_tasks.push(task);
    }

    Ok(Tasks {
      templates: templates,
      tasks: new_tasks,
    })
  }

  /// Create a new `Tasks` object from a serializable one without any tags.
  #[cfg(test)]
  pub fn with_serde_tasks(tasks: Vec<SerTask>) -> Result<Self> {
    // Test code using this constructor is assumed to only have tasks
    // that have no tags.
    tasks.iter().for_each(|x| assert!(x.tags.is_empty()));

    let templates = Rc::new(Templates::new());
    let map = Default::default();

    Self::with_serde(SerTasks(tasks), templates, &map)
  }

  /// Convert this object into a serializable one.
  pub fn to_serde(&self) -> SerTasks {
    SerTasks(self.tasks.iter().map(|x| x.to_serde()).collect())
  }

  /// Retrieve an iterator over the tasks.
  pub fn iter(&self) -> TaskIter {
    self.tasks.iter()
  }

  /// Add a new task.
  pub fn add(&mut self, summary: String, tags: Vec<Tag>) -> Id {
    let task = Task::with_summary_and_tags(summary, tags, self.templates.clone());
    let id = task.id;
    self.tasks.push(task);
    id
  }

  /// Remove a task.
  pub fn remove(&mut self, id: Id) {
    self
      .tasks
      .iter()
      .position(|x| x.id() == id)
      .map(|x| self.tasks.remove(x))
      .unwrap();
  }

  /// Update a task.
  pub fn update(&mut self, task: Task) {
    self
      .tasks
      .iter_mut()
      .position(|x| x.id() == task.id())
      .map(|x| self.tasks[x] = task)
      .unwrap();
  }
}


#[cfg(test)]
pub mod tests {
  use super::*;

  use serde_json::from_str as from_json;
  use serde_json::to_string_pretty as to_json;

  use ser::tags::Templates as SerTemplates;
  use test::make_tasks;


  #[test]
  fn add_task() {
    let mut tasks = Tasks::with_serde_tasks(make_tasks(3)).unwrap();
    let tags = Default::default();
    tasks.add("4".to_string(), tags);

    assert_eq!(tasks.to_serde().0, make_tasks(4));
  }

  #[test]
  fn remove_task() {
    let mut tasks = Tasks::with_serde_tasks(make_tasks(3)).unwrap();
    let id = tasks.iter().nth(1).unwrap().id();
    tasks.remove(id);

    let mut expected = make_tasks(3);
    expected.remove(1);

    assert_eq!(tasks.to_serde().0, expected);
  }

  #[test]
  fn update_task() {
    let mut tasks = Tasks::with_serde_tasks(make_tasks(3)).unwrap();
    let mut task = tasks.iter().nth(1).unwrap().clone();
    task.summary = "amended".to_string();
    tasks.update(task);

    let mut expected = make_tasks(3);
    expected[1].summary = "amended".to_string();

    assert_eq!(tasks.to_serde().0, expected);
  }

  #[test]
  fn task_completion() {
    let mut task = Task::new("test task");
    assert!(!task.is_complete());
    task.toggle_complete();
    assert!(task.is_complete());
  }

  #[test]
  fn serialize_deserialize_task() {
    let task = Task::new("this is a TODO");
    let serialized = to_json(&task.to_serde()).unwrap();
    let deserialized = from_json::<SerTask>(&serialized).unwrap();

    assert_eq!(deserialized.summary, task.summary);
  }

  #[test]
  fn serialize_deserialize_tasks() {
    let (templates, map) = Templates::with_serde(SerTemplates(Default::default()));
    let templates = Rc::new(templates);
    let tasks = Tasks::with_serde_tasks(make_tasks(3)).unwrap();
    let serialized = to_json(&tasks.to_serde()).unwrap();
    let deserialized = from_json::<SerTasks>(&serialized).unwrap();
    let tasks = Tasks::with_serde(deserialized, templates, &map).unwrap();

    assert_eq!(tasks.to_serde().0, make_tasks(3));
  }
}
