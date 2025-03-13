//! Core interfaces for builders.
//!
//! These interfaces help to organize building process as a pipeline of steps.
//! See [`Step`] and [`Pipeline`].

use anyhow::Result;
use std::any::Any;
use std::collections::HashMap;

/// Step in the pipeline.
pub trait Step<Ctx> {
    /// Run step.
    fn run(&mut self, ctx: &mut Ctx) -> Result<()>;
}

/// Steps of the pipeline.
pub type Steps<Ctx> = Vec<Box<dyn Step<Ctx>>>;

/// Direct pipeline, running step in order of appearance.
pub struct Pipeline<'ctx, Ctx> {
    ctx: &'ctx mut Ctx,
    steps: Steps<Ctx>,
}

impl<'ctx, Ctx> Pipeline<'ctx, Ctx> {
    /// Create new pipiline with given context.
    pub fn from_ctx(ctx: &'ctx mut Ctx) -> Self {
        Self {
            ctx,
            steps: Vec::new(),
        }
    }

    /// Add step to the end of pipeline.
    pub fn add_step(&mut self, step: Box<dyn Step<Ctx>>) {
        self.steps.push(step);
    }

    /// Add steps to the end of pipeline.
    pub fn add_steps<I>(&mut self, steps: I)
    where
        I: IntoIterator<Item = Box<dyn Step<Ctx>>>,
    {
        for step in steps.into_iter() {
            self.add_step(step);
        }
    }

    /// Create new pipeline with given context and steps.
    pub fn from_steps<I>(ctx: &'ctx mut Ctx, steps: I) -> Self
    where
        I: IntoIterator<Item = Box<dyn Step<Ctx>>>,
    {
        let mut pipeline = Self::from_ctx(ctx);
        pipeline.add_steps(steps);
        pipeline
    }

    /// Run pipeline.
    pub fn run(mut self) -> Result<()> {
        for mut step in self.steps {
            step.run(&mut self.ctx)?;
        }
        Ok(())
    }
}

/// Heterogeneous context for pipeline.
pub struct Context {
    inner: HashMap<&'static str, Box<dyn Any>>,
}

impl Context {
    /// New empty context.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Get reference to value by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn get<T>(&self, key: &'static str) -> Option<&T>
    where
        T: 'static,
    {
        self.inner.get(key).map(|t| t.downcast_ref::<T>()).flatten()
    }

    /// Get mutable reference to value by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn get_mut<T>(&mut self, key: &'static str) -> Option<&mut T>
    where
        T: 'static,
    {
        self.inner
            .get_mut(key)
            .map(|t| t.downcast_mut::<T>())
            .flatten()
    }

    /// Pop value from context by key. `T` is a downcast type of value.
    /// Returns `None` if `key` doesn't exists or downcast type is wrong.
    pub fn pop<T>(&mut self, key: &'static str) -> Option<Box<T>>
    where
        T: 'static,
    {
        self.inner
            .remove(key)
            .map(|t| t.downcast::<T>().ok())
            .flatten()
    }

    /// Set value for the key.
    pub fn set(&mut self, key: &'static str, value: Box<dyn Any>) {
        self.inner.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{Context, Pipeline, Result, Step};

    struct Step1;

    impl Step<String> for Step1 {
        fn run(&mut self, ctx: &mut String) -> Result<()> {
            ctx.push_str("step1");
            Ok(())
        }
    }

    struct Step2;

    impl Step<String> for Step2 {
        fn run(&mut self, ctx: &mut String) -> Result<()> {
            ctx.contains("step1")
                .then(|| {})
                .ok_or(anyhow::anyhow!("step2 failed"))
        }
    }

    #[test]
    pub fn test_pipeline_ok() {
        let mut ctx = String::new();
        let mut pipeline = Pipeline::from_ctx(&mut ctx);
        pipeline.add_step(Box::new(Step1));
        pipeline.add_step(Box::new(Step2));
        pipeline.run().expect("run must be okay");
    }

    #[test]
    pub fn test_pipeline_fail() {
        let mut ctx = String::new();
        let mut pipeline = Pipeline::from_ctx(&mut ctx);
        pipeline.add_step(Box::new(Step2));
        let result = pipeline.run();
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert_eq!(&err, "step2 failed");
    }

    struct Step1Context {
        inner: String,
    }

    #[test]
    pub fn test_context() {
        let mut ctx = Context::new();
        ctx.set(
            "step1",
            Box::new(Step1Context {
                inner: "content".to_string(),
            }),
        );
        let ctx1 = ctx
            .get_mut::<Step1Context>("step1")
            .expect("step1 context must exist");
        assert_eq!(&ctx1.inner, "content");
        ctx1.inner = "new content".to_string();

        let ctx1 = ctx
            .get::<Step1Context>("step1")
            .expect("step1 context must exist");
        assert_eq!(&ctx1.inner, "new content");
    }
}
