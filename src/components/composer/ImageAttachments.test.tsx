// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ImageAttachments } from './ImageAttachments';

describe('ImageAttachments', () => {
  it('renders images and calls onRemove on the remove button', () => {
    const onRemove = vi.fn();
    render(
      <ImageAttachments
        images={[{ id: 'i1', dataUrl: 'data:image/png;base64,x', name: 'a.png' }]}
        onRemove={onRemove}
      />,
    );
    expect(screen.getByRole('img')).toBeTruthy();
    fireEvent.click(screen.getByLabelText('Remove a.png'));
    expect(onRemove).toHaveBeenCalledWith('i1');
  });

  it('renders nothing when there are no images', () => {
    const { container } = render(<ImageAttachments images={[]} />);
    expect(container.firstChild).toBeNull();
  });
});
