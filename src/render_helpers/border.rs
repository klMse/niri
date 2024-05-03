use std::collections::HashMap;

use glam::{Mat3, Vec2};
use niri_config::CornerRadius;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{GlesError, GlesFrame, GlesRenderer, Uniform};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet};
use smithay::utils::{Buffer, Logical, Physical, Rectangle, Scale, Transform};

use super::renderer::NiriRenderer;
use super::shader_element::{ShaderProgram, ShaderRenderElement};
use super::shaders::{mat3_uniform, Shaders};
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};

/// Renders a wide variety of borders and border parts.
///
/// This includes:
/// * sub- or super-rect of an angled linear gradient like CSS linear-gradient(angle, a, b).
/// * corner rounding.
/// * as a background rectangle and as parts of a border line.
#[derive(Debug, Clone)]
pub struct BorderRenderElement {
    inner: ShaderRenderElement,
    params: Parameters,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Parameters {
    scale: Scale<f64>,
    area: Rectangle<i32, Logical>,
    gradient_area: Rectangle<i32, Logical>,
    color_from: [f32; 4],
    color_to: [f32; 4],
    angle: f32,
    geometry: Rectangle<i32, Logical>,
    border_width: f32,
    corner_radius: CornerRadius,
}

impl BorderRenderElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shader: &ShaderProgram,
        scale: Scale<f64>,
        area: Rectangle<i32, Logical>,
        gradient_area: Rectangle<i32, Logical>,
        color_from: [f32; 4],
        color_to: [f32; 4],
        angle: f32,
        geometry: Rectangle<i32, Logical>,
        border_width: f32,
        corner_radius: CornerRadius,
    ) -> Self {
        let mut inner = ShaderRenderElement::empty(Kind::Unspecified);
        inner.update_shader(Some(shader));
        let mut rv = Self {
            inner,
            params: Parameters {
                scale,
                area,
                gradient_area,
                color_from,
                color_to,
                angle,
                geometry,
                border_width,
                corner_radius,
            },
        };
        rv.update_inner();
        rv
    }

    pub fn empty() -> Self {
        let inner = ShaderRenderElement::empty(Kind::Unspecified);
        Self {
            inner,
            params: Parameters {
                scale: Scale::from(1.),
                area: Default::default(),
                gradient_area: Default::default(),
                color_from: Default::default(),
                color_to: Default::default(),
                angle: 0.,
                geometry: Default::default(),
                border_width: 0.,
                corner_radius: Default::default(),
            },
        }
    }

    pub fn update_shader(&mut self, shader: Option<&ShaderProgram>) {
        self.inner.update_shader(shader);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        scale: Scale<f64>,
        area: Rectangle<i32, Logical>,
        gradient_area: Rectangle<i32, Logical>,
        color_from: [f32; 4],
        color_to: [f32; 4],
        angle: f32,
        geometry: Rectangle<i32, Logical>,
        border_width: f32,
        corner_radius: CornerRadius,
    ) {
        let params = Parameters {
            scale,
            area,
            gradient_area,
            color_from,
            color_to,
            angle,
            geometry,
            border_width,
            corner_radius,
        };
        if self.params == params {
            return;
        }

        self.params = params;
        self.update_inner();
    }

    fn update_inner(&mut self) {
        let Parameters {
            scale,
            area,
            gradient_area,
            color_from,
            color_to,
            angle,
            geometry,
            border_width,
            corner_radius,
        } = self.params;

        let grad_offset = (area.loc - gradient_area.loc).to_f64().to_physical(scale);

        let grad_dir = Vec2::from_angle(angle);

        let grad_area_size = gradient_area.size.to_f64().to_physical(scale);
        let (w, h) = (grad_area_size.w as f32, grad_area_size.h as f32);

        let mut grad_area_diag = Vec2::new(w, h);
        if (grad_dir.x < 0. && 0. <= grad_dir.y) || (0. <= grad_dir.x && grad_dir.y < 0.) {
            grad_area_diag.x = -w;
        }

        let mut grad_vec = grad_area_diag.project_onto(grad_dir);
        if grad_dir.y <= 0. {
            grad_vec = -grad_vec;
        }

        let area_physical = area.to_physical_precise_round(scale);
        let area_loc = Vec2::new(area_physical.loc.x, area_physical.loc.y);
        let area_size = Vec2::new(area_physical.size.w, area_physical.size.h);

        let geo = geometry.to_physical_precise_round(scale);
        let geo_loc = Vec2::new(geo.loc.x, geo.loc.y);
        let geo_size = Vec2::new(geo.size.w, geo.size.h);

        let input_to_geo =
            Mat3::from_scale(area_size) * Mat3::from_translation((area_loc - geo_loc) / area_size);
        let corner_radius = corner_radius.scaled_by(scale.x as f32);
        let border_width = border_width * scale.x as f32;

        self.inner.update(
            area,
            area.size.to_f64().to_buffer(scale, Transform::Normal),
            None,
            vec![
                Uniform::new("color_from", color_from),
                Uniform::new("color_to", color_to),
                Uniform::new("grad_offset", (grad_offset.x as f32, grad_offset.y as f32)),
                Uniform::new("grad_width", w),
                Uniform::new("grad_vec", grad_vec.to_array()),
                mat3_uniform("input_to_geo", input_to_geo),
                Uniform::new("geo_size", geo_size.to_array()),
                Uniform::new("outer_radius", <[f32; 4]>::from(corner_radius)),
                Uniform::new("border_width", border_width),
            ],
            HashMap::new(),
        );
    }

    pub fn has_shader(&self) -> bool {
        self.inner.has_shader()
    }

    pub fn shader(renderer: &mut impl NiriRenderer) -> Option<&ShaderProgram> {
        Shaders::get(renderer).border.as_ref()
    }
}

impl Default for BorderRenderElement {
    fn default() -> Self {
        Self::empty()
    }
}

impl Element for BorderRenderElement {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> Vec<Rectangle<i32, Physical>> {
        self.inner.opaque_regions(scale)
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for BorderRenderElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        RenderElement::<GlesRenderer>::draw(&self.inner, frame, src, dst, damage)
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage> {
        self.inner.underlying_storage(renderer)
    }
}

impl<'render> RenderElement<TtyRenderer<'render>> for BorderRenderElement {
    fn draw(
        &self,
        frame: &mut TtyFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
    ) -> Result<(), TtyRendererError<'render>> {
        RenderElement::<TtyRenderer<'_>>::draw(&self.inner, frame, src, dst, damage)
    }

    fn underlying_storage(&self, renderer: &mut TtyRenderer<'render>) -> Option<UnderlyingStorage> {
        self.inner.underlying_storage(renderer)
    }
}