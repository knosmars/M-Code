import { Component, type ReactNode } from 'react';
import styles from './ErrorBoundary.module.css';

type FallbackRender = (error: Error, retry: () => void) => ReactNode;

interface ErrorBoundaryProps {
  /** Custom fallback UI or render function (receives error + retry). */
  fallback?: ReactNode | FallbackRender;
  /** Optional callback to log or report errors. */
  onError?: (error: Error, errorInfo: string) => void;
  children: ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

/**
 * React error boundary that catches rendering errors in its subtree.
 *
 * Prevents the entire app from crashing when a child component throws.
 * Wrap individual message bubbles and the main content area.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: { componentStack: string }): void {
    const stack = errorInfo.componentStack ?? '';
    console.error('[ErrorBoundary] Caught rendering error:', error.message, stack);
    this.props.onError?.(error, stack);
  }

  handleRetry = (): void => {
    this.setState({ hasError: false, error: null });
  };

  render(): ReactNode {
    if (this.state.hasError) {
      if (this.props.fallback) {
        if (typeof this.props.fallback === 'function') {
          return (this.props.fallback as FallbackRender)(this.state.error!, this.handleRetry);
        }
        return this.props.fallback;
      }

      return (
        <div className={styles.fallback}>
          <div className={styles.title}>Something went wrong</div>
          <div className={styles.message}>
            {this.state.error?.message ?? 'An unexpected rendering error occurred.'}
          </div>
          <button
            type="button"
            className={styles.retry}
            onClick={this.handleRetry}
          >
            Try Again
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
