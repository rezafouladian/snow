use anyhow::Result;
use crate::renderer::Renderer;
use crate::tickable::{Tickable, Ticks};

pub struct Video<T: Renderer> {
    renderer: T,

    pub framebuffer: Vec<u8>,
    vblank_ticks: Ticks,
}

impl<T> Video<T>
where
    T: Renderer,
{
    /// Display width in pixels
    const DISPLAY_WIDTH: usize = 640;
    /// Display height in pixels
    const DISPLAY_HEIGHT: usize = 400;

    const FRAMEBUFFER_SIZE: usize = (Self::DISPLAY_WIDTH * Self::DISPLAY_HEIGHT) / 8;

    pub fn new(renderer: T) -> Self {
        Self {
            renderer,
            framebuffer: vec![0xFF; Self::FRAMEBUFFER_SIZE],
            vblank_ticks: 0,
        }
    }

    fn render(&mut self) -> Result<()> {
        let fb = &self.framebuffer;

        let buf = self.renderer.buffer_mut();
        buf.set_size(Self::DISPLAY_WIDTH, Self::DISPLAY_HEIGHT);

        self.renderer.update()?;

        Ok(())
    }

    pub(crate) fn blank(&mut self) -> Result<()> {
        self.framebuffer.fill(0xFF);
        self.render()
    }
}

impl<T> Tickable for Video<T>
where
    T: Renderer,
{
    fn tick(&mut self, ticks: Ticks) -> Result<Ticks> {
        self.vblank_ticks += ticks;
        if self.vblank_ticks > 16_000_000 /60 {
            self.render()?;

            self.vblank_ticks = 0;
        }
        Ok(ticks)
    }
}